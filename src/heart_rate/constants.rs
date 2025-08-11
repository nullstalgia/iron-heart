pub mod ble_uuids {
    
    pub mod service {
        use uuid::Uuid;
        pub use btuuid::service::*;

        pub const MIBAND_AUTH: Uuid =
            Uuid::from_u128(0x0000fee1_0000_1000_8000_00805f9b34fb); // 0000fee1-0000-1000-8000-00805f9b34fb
        pub const MIBAND_SENSOR: Uuid =
            Uuid::from_u128(0x0000fee0_0000_1000_8000_00805f9b34fb); // 0000fee0-0000-1000-8000-00805f9b34fb
    }
    pub mod characteristic {
        use uuid::Uuid;
        pub use btuuid::characteristic::*;
        
        pub const MIBAND_AUTH: Uuid =
            Uuid::from_u128(0x00000009_0000_3512_2118_0009af100700); // 00000009-0000-3512-2118-0009af100700
        pub const MIBAND_SENSOR: Uuid =
            Uuid::from_u128(0x00000001_0000_3512_2118_0009af100700); // 00000001-0000-3512-2118-0009af100700
    }
}