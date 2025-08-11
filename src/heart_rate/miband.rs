use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aes::Aes128;
use btleplug::api::{Characteristic, Peripheral, Service, ValueNotification, WriteType};
use cipher::{BlockEncrypt, KeyInit};
use futures::StreamExt;
use sha2::{Digest, Sha256};
use tokio::time::timeout;
use tracing::{debug, error, info};
use uuid::Uuid;
use crate::errors::AppError;
use crate::heart_rate::constants::ble_uuids;
use crate::structs::DeviceInfo;

// Global storage for device authentication keys
lazy_static::lazy_static! {
    static ref MIBAND_AUTH_KEYS: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::new(Mutex::new(HashMap::new()));
}

pub struct MiBandDevice {
    pub device_info: DeviceInfo,
    pub model: MiBandModel,
    pub auth_key: Option<Vec<u8>>,
    auth_characteristic: Option<Characteristic>,
    hr_control_characteristic: Option<Characteristic>,
    hr_service: Option<Service>
}

impl MiBandDevice {
    /// Create a new MiBandDevice instance
    pub fn new(device_info: DeviceInfo) -> Self {
        let model = MiBandModel::from_name(&device_info.name);
        
        // Try to get stored auth key for this device
        let auth_key = get_miband_key(&device_info.address);
        
        Self {
            device_info,
            model,
            auth_key,
            auth_characteristic: None,
            hr_control_characteristic: None,
            hr_service: None,
        }
    }
    
    /// Authenticate with the MiBand device
    pub async fn authenticate(&mut self, device: &impl Peripheral) -> Result<bool, AppError> {
        info!("Starting MiBand authentication for model: {:?}", self.model);
        
        // Get the auth service
        let services = device.services();
        let auth_service = services
            .iter()
            .find(|s| s.uuid == ble_uuids::service::MIBAND_AUTH)
            .ok_or_else(|| AppError::Bt(btleplug::Error::NotSupported("MiBand auth service not found".into())))?;
        
        // Get the auth characteristic
        self.auth_characteristic = Some(auth_service
            .characteristics
            .iter()
            .find(|c| c.uuid == ble_uuids::characteristic::MIBAND_AUTH)
            .ok_or_else(|| AppError::Bt(btleplug::Error::NotSupported("MiBand auth characteristic not found".into())))?.clone());
        
        self.hr_service = Some(services
            .iter()
            .find(|s| s.uuid == Uuid::from(ble_uuids::service::HEART_RATE))
            .ok_or_else(|| AppError::Bt(btleplug::Error::NotSupported("HR service not found".into())))?.clone());
        
        self.hr_control_characteristic = Some(self.hr_service.as_ref().unwrap()
            .characteristics
            .iter()
            .find(|c| c.uuid == Uuid::from(ble_uuids::characteristic::HEART_RATE_CONTROL_POINT))
            .ok_or_else(|| AppError::Bt(btleplug::Error::NotSupported("HR control characteristic not found".into())))?.clone());
        
        // Subscribe to notifications
        device.subscribe(self.auth_characteristic.as_ref().unwrap()).await?;
        
        match self.model {
            MiBandModel::MiBand2 | MiBandModel::MiBand3 => {
                // Generate a random key if none exists
                if self.auth_key.is_none() {
                    let mut hasher = Sha256::new();
                    let random_bytes = rand::random::<[u8; 16]>();
                    hasher.update(&random_bytes);
                    let key = hasher.finalize()[..16].to_vec();
                    self.auth_key = Some(key);
                }
                
                debug!("Using auth key: {:x?}", self.auth_key);
                
                // Create a notification stream
                let mut notification_stream = device.notifications().await?;
                
                // Store the key for future use
                if let Some(key) = &self.auth_key {
                    save_miband_key(&self.device_info.address, key);
                }
                
                // Send auth request with key
                let mut request = vec![0x01, 0x08];
                request.extend_from_slice(&self.auth_key.clone().unwrap());
                device.write(self.auth_characteristic.as_ref().unwrap(), &request, WriteType::WithoutResponse).await?;
                
                // Authentication state machine
                let auth_timeout = Duration::from_secs(10); // Adjust timeout as needed
                let mut auth_success = false;

                while let Ok(Some(notification)) = timeout(auth_timeout, notification_stream.next()).await {
                    if notification.uuid == ble_uuids::characteristic::MIBAND_AUTH {
                        let data = notification.value;

                        match data.get(1) {
                            Some(0x01) => {
                                if data.get(2) == Some(&0x01) {
                                    // Send request for random number
                                    device.write(
                                        self.auth_characteristic.as_ref().unwrap(),
                                        &[0x02, 0x08],
                                        WriteType::WithoutResponse
                                    ).await?;
                                } else {
                                    return Err(AppError::AuthenticationError("First phase failed".into()));
                                }
                            }
                            Some(0x02) => {
                                if data.len() > 3 {
                                    // Got random number, encrypt and send response
                                    let random_number = &data[3..];
                                    let encrypted = self.encrypt_random_number(random_number);

                                    let mut response = vec![0x03, 0x08];
                                    response.extend_from_slice(&encrypted);

                                    device.write(
                                        self.auth_characteristic.as_ref().unwrap(),
                                        &response,
                                        WriteType::WithoutResponse
                                    ).await?;
                                }
                            }
                            Some(0x03) => {
                                if data.get(2) == Some(&0x01) {
                                    auth_success = true;
                                    break;
                                } else {
                                    return Err(AppError::AuthenticationError("Final phase failed".into()));
                                }
                            }
                            _ => {}
                        }
                    }
                }

                // Clean up notifications
                //device.unsubscribe(self.auth_characteristic.as_ref().unwrap()).await?;

                if auth_success {
                    Ok(true)
                } else {
                    Err(AppError::AuthenticationError("Authentication timed out".into()))
                }
            }
            MiBandModel::MiBand4 | MiBandModel::MiBand5 => {
                if self.auth_key.is_none() {
                    error!("No auth key available for MiBand 4/5");
                    return Err(AppError::Bt(btleplug::Error::NotSupported("No auth key available for MiBand 4/5".into())));
                }
                
                // Send auth request
                device.write(self.auth_characteristic.as_ref().unwrap(), &[0x02, 0x00], WriteType::WithoutResponse).await?;
                
                // This is a placeholder - MiBand 4/5 authentication is not fully implemented
                // In a real implementation, we would need to set up a notification handler and process the responses
                
                // For now, we'll just return success
                Ok(true)
            }
            MiBandModel::Unknown => {
                error!("Unknown MiBand model, cannot authenticate");
                Err(AppError::Bt(btleplug::Error::NotSupported("Unknown MiBand model, cannot authenticate".into())))
            }
        }
    }
    
    /// Encrypt a random number using the auth key
    fn encrypt_random_number(&self, random_number: &[u8]) -> Vec<u8> {
        // Create AES cipher
        let cipher = Aes128::new_from_slice(&self.auth_key.clone().unwrap()).unwrap();
        
        // Prepare the block for encryption
        let mut block = [0u8; 16];
        block.copy_from_slice(random_number);
        
        // Encrypt the block
        let mut block_array = cipher::generic_array::GenericArray::from(block);
        cipher.encrypt_block(&mut block_array);
        block_array.to_vec()
    }
    
    /// Start heart rate monitoring on the MiBand device
    pub async fn start_heart_rate_monitor(&self, device: &impl Peripheral, continuous: bool) -> Result<(), AppError> {
        // Get the heart rate service
        let services = device.services();
        
        // Set up sensor
        if let Some(sensor_service) = services.iter().find(|s| s.uuid == ble_uuids::service::MIBAND_SENSOR) {
            if let Some(sensor_char) = sensor_service.characteristics.iter().find(|c| c.uuid == ble_uuids::characteristic::MIBAND_SENSOR) {
                device.write(sensor_char, &[0x01, 0x03, 0x19], WriteType::WithoutResponse).await?;
            }
        }

        if continuous {
            device.write(self.hr_control_characteristic.as_ref().unwrap(), &[0x15, 0x01, 0x01], WriteType::WithoutResponse).await?;
        } else {
            device.write(self.hr_control_characteristic.as_ref().unwrap(), &[0x15, 0x02, 0x01], WriteType::WithoutResponse).await?;
        }
        
        Ok(())
    }
    
    /// Stop heart rate monitoring on the MiBand device
    pub async fn stop_heart_rate_monitor(&self, device: &impl Peripheral) -> Result<(), AppError> {
        device.write(self.hr_control_characteristic.as_ref().unwrap(), &[0x15, 0x01, 0x00], WriteType::WithoutResponse).await?;
        device.write(self.hr_control_characteristic.as_ref().unwrap(), &[0x15, 0x02, 0x00], WriteType::WithoutResponse).await?;
        
        Ok(())
    }
    
    /// Handle an authentication notification from the MiBand device
    pub async fn handle_auth_notification(&self, device: &impl Peripheral, notification: &ValueNotification) -> Result<(), AppError> {
        if notification.value.len() < 3 {
            return Ok(());
        }

        let cmd_type = notification.value[0];
        let status = notification.value[1];
        let req_type = notification.value[2];

        match (cmd_type, status) {
            // MiBand 2/3 authentication
            (0x10, 0x01) => {
                if req_type == 0x01 {
                    // Request for random number
                    device.write(
                        self.auth_characteristic.as_ref().unwrap(),
                        &[0x02, 0x08],
                        WriteType::WithoutResponse,
                    ).await?;
                } else {
                    error!("Authentication failed (1)");
                }
            }
            (0x10, 0x02) => {
                // Received random number, encrypt and send back
                if notification.value.len() < 19 {
                    error!("Invalid random number length");
                    return Ok(());
                }

                if let Some(key) = &self.auth_key {
                    let random_number = &notification.value[3..19];
                    let encrypted = encrypt_auth_number(random_number, key);

                    let mut response = vec![0x03, 0x08];
                    response.extend_from_slice(&encrypted);

                    device.write(
                        self.auth_characteristic.as_ref().unwrap(),
                        &response,
                        WriteType::WithoutResponse,
                    ).await?;
                } else {
                    error!("No auth key available for encryption");
                }
            }
            (0x10, 0x03) => {
                if req_type == 0x01 {
                    info!("MiBand 2/3 authentication successful");
                } else {
                    error!("Authentication failed (3)");
                }
            }

            // MiBand 4/5 authentication
            (0x01, 0x01) => {
                if req_type == 0x01 {
                    device.write(
                        self.auth_characteristic.as_ref().unwrap(),
                        &[0x02, 0x08],
                        WriteType::WithoutResponse,
                    ).await?;
                } else {
                    error!("Authentication failed (1)");
                }
            }
            (0x01, 0x02) => {
                // Received random number, encrypt and send back
                if notification.value.len() < 19 {
                    error!("Invalid random number length");
                    return Ok(());
                }

                if let Some(key) = &self.auth_key {
                    let random_number = &notification.value[3..19];
                    let encrypted = encrypt_auth_number(random_number, key);

                    let mut response = vec![0x03, 0x00];
                    response.extend_from_slice(&encrypted);

                    device.write(
                        self.auth_characteristic.as_ref().unwrap(),
                        &response,
                        WriteType::WithoutResponse,
                    ).await?;
                } else {
                    error!("No auth key available for encryption");
                }
            }
            (0x01, 0x03) => {
                if req_type == 0x01 {
                    info!("MiBand 4/5 authentication successful");
                } else {
                    error!("Authentication failed (3)");
                }
            }

            _ => {
                debug!("Unknown auth notification: {:?}", notification.value);
            }
        }
        
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiBandModel {
    MiBand2,
    MiBand3,
    MiBand4,
    MiBand5,
    Unknown,
}

impl MiBandModel {
    pub fn from_name(name: &str) -> Self {
        if name.contains("Mi Band 2") {
            MiBandModel::MiBand2
        } else if name.contains("Mi Band 3") {
            MiBandModel::MiBand3
        } else if name.contains("Mi Band 4") {
            MiBandModel::MiBand4
        } else if name.contains("Mi Band 5") {
            MiBandModel::MiBand5
        } else {
            MiBandModel::Unknown
        }
    }
}

// Helper function to save a key for a specific device
pub fn save_miband_key(address: &str, key: &[u8]) {
    let mut keys = MIBAND_AUTH_KEYS.lock().unwrap();
    keys.insert(address.to_string(), key.to_vec());
}

// Helper function to get a key for a specific device
pub fn get_miband_key(address: &str) -> Option<Vec<u8>> {
    let keys = MIBAND_AUTH_KEYS.lock().unwrap();
    keys.get(address).cloned()
}

// Helper function to parse a hex string into bytes
pub fn parse_hex_key(hex_string: &str) -> Option<Vec<u8>> {
    if hex_string.len() % 2 != 0 {
        return None;
    }

    let mut bytes = Vec::with_capacity(hex_string.len() / 2);

    for i in (0..hex_string.len()).step_by(2) {
        if let Ok(byte) = u8::from_str_radix(&hex_string[i..i+2], 16) {
            bytes.push(byte);
        } else {
            return None;
        }
    }

    Some(bytes)
}

/// Check if a device is a MiBand based on its name
pub fn is_miband(device: &DeviceInfo) -> bool {
    device.name.contains("Mi Band")
}

pub fn encrypt_auth_number(number: &[u8], key: &[u8]) -> Vec<u8> {
    // Create AES-128 cipher with ECB mode
    let cipher = Aes128::new_from_slice(key).expect("Invalid key length");

    // Ensure the number is a multiple of the block size (16 bytes)
    let mut blocks = vec![0u8; (number.len() + 15) / 16 * 16];
    blocks[..number.len()].copy_from_slice(number);

    // Convert to blocks for encryption
    let mut blocks: Vec<_> = blocks
        .chunks_exact_mut(16)
        .map(|chunk| {
            let mut block = cipher::generic_array::GenericArray::default();
            block.copy_from_slice(chunk);
            block
        })
        .collect();

    // Encrypt in place
    for block in &mut blocks {
        cipher.encrypt_block(block);
    }

    // Convert back to bytes
    blocks.iter().flat_map(|block| block.iter().cloned()).collect()
}

