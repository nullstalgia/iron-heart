use rosc::address::verify_address;

use crate::{errors::AppError, settings::OscSettings};

pub(super) struct OscAddresses {
    pub beat_toggle: String,
    pub beat_pulse: String,
    pub bpm_int: String,
    pub bpm_float: String,
    pub connected: String,
    pub hiding_disconnect: String,
    pub latest_rr: String,
    pub battery_int: String,
    pub battery_float: String,
    pub rr_twitch_up: String,
    pub rr_twitch_down: String,
}

// Not sure if rosc has a function for this already
fn remove_double_slashes(address: &mut String) {
    while let Some(pos) = address.find("//") {
        address.replace_range(pos..pos + 2, "/");
    }
}

fn remove_trailing_char(s: &mut String, ch: char) {
    if s.len() > 1 && s.ends_with(ch) {
        s.pop();
    }
}

fn format_prefix(prefix: &str) -> Result<String, AppError> {
    let mut address = String::from("/");
    address.push_str(prefix);
    remove_double_slashes(&mut address);
    remove_trailing_char(&mut address, '/');
    if verify_address(&address).is_ok() || address == "/" {
        Ok(address)
    } else {
        Err(AppError::OscPrefix(prefix.to_owned()))
    }
}

fn format_address(prefix: &str, param: &str, param_name: &str) -> Result<String, AppError> {
    // Don't allow empty/blank addresses
    if param.len() == 0 || param == "/" {
        return Err(AppError::OscAddress(
            param_name.to_owned(),
            param.to_owned(),
        ));
    }
    let mut address = format!("{prefix}/{param}");
    remove_double_slashes(&mut address);
    remove_trailing_char(&mut address, '/');
    if verify_address(&address).is_ok() {
        Ok(address)
    } else {
        Err(AppError::OscAddress(
            param_name.to_owned(),
            param.to_owned(),
        ))
    }
}

impl OscAddresses {
    pub fn build(osc_settings: &OscSettings) -> Result<Self, AppError> {
        let prefix = format_prefix(&osc_settings.address_prefix)?;
        Ok(OscAddresses {
            beat_toggle: format_address(
                &prefix,
                &osc_settings.param_beat_toggle,
                "param_beat_toggle",
            )?,
            beat_pulse: format_address(
                &prefix,
                &osc_settings.param_beat_pulse,
                "param_beat_pulse",
            )?,
            bpm_int: format_address(&prefix, &osc_settings.param_bpm_int, "param_bpm_int")?,
            bpm_float: format_address(&prefix, &osc_settings.param_bpm_float, "param_bpm_float")?,
            connected: format_address(
                &prefix,
                &osc_settings.param_hrm_connected,
                "param_hrm_connected",
            )?,
            hiding_disconnect: format_address(
                &prefix,
                &osc_settings.param_hiding_disconnect,
                "param_hiding_disconnect",
            )?,
            latest_rr: format_address(
                &prefix,
                &osc_settings.param_latest_rr_int,
                "param_latest_rr_int",
            )?,
            battery_int: format_address(
                &prefix,
                &osc_settings.param_hrm_battery_int,
                "param_hrm_battery_int",
            )?,
            battery_float: format_address(
                &prefix,
                &osc_settings.param_hrm_battery_float,
                "param_hrm_battery_float",
            )?,
            rr_twitch_up: format_address(
                &prefix,
                &osc_settings.param_rr_twitch_up,
                "param_rr_twitch_up",
            )?,
            rr_twitch_down: format_address(
                &prefix,
                &osc_settings.param_rr_twitch_down,
                "param_rr_twitch_down",
            )?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_missing_leading_slash() -> Result<(), AppError> {
        let potential_input = "avatar/parameters";
        let ideal_output = "/avatar/parameters";
        let test = format_prefix(potential_input)?;
        assert_eq!(test, ideal_output);
        Ok(())
    }
    #[test]
    fn prefix_extra_proceeding_slash() -> Result<(), AppError> {
        let potential_input = "/avatar/parameters/";
        let ideal_output = "/avatar/parameters";
        let test = format_prefix(potential_input)?;
        assert_eq!(test, ideal_output);
        Ok(())
    }
    #[test]
    fn prefix_malformed_slashes() -> Result<(), AppError> {
        let potential_input = "avatar///parameters/";
        let ideal_output = "/avatar/parameters";
        let test = format_prefix(potential_input)?;
        assert_eq!(test, ideal_output);
        Ok(())
    }
    #[test]
    fn prefix_empty() -> Result<(), AppError> {
        let potential_input = "";
        let ideal_output = "/";
        let test = format_prefix(potential_input)?;
        assert_eq!(test, ideal_output);
        Ok(())
    }
    #[test]
    fn prefix_slash() -> Result<(), AppError> {
        let potential_input = "/";
        let ideal_output = "/";
        let test = format_prefix(potential_input)?;
        assert_eq!(test, ideal_output);
        Ok(())
    }
    #[test]
    fn prefix_many_slashes() -> Result<(), AppError> {
        let ideal_output = "/";
        let mut potential_input = "//";
        let mut test = format_prefix(potential_input)?;
        assert_eq!(test, ideal_output);

        potential_input = "///";
        test = format_prefix(potential_input)?;
        assert_eq!(test, ideal_output);

        potential_input = "////";
        test = format_prefix(potential_input)?;
        assert_eq!(test, ideal_output);
        Ok(())
    }

    #[test]
    fn address_extra_leading_slash() -> Result<(), AppError> {
        let healthy_prefix = "/avatar/parameters";
        let potential_addr = "/hr";

        let ideal_output = "/avatar/parameters/hr";
        let test = format_address(
            healthy_prefix,
            potential_addr,
            "address_extra_leading_slash",
        )?;
        assert_eq!(test, ideal_output);
        Ok(())
    }
    #[test]
    fn address_extra_proceeding_slash() -> Result<(), AppError> {
        let healthy_prefix = "/avatar/parameters";
        let potential_addr = "hr/test/";

        let ideal_output = "/avatar/parameters/hr/test";
        let test = format_address(
            healthy_prefix,
            potential_addr,
            "address_extra_proceeding_slash",
        )?;
        assert_eq!(test, ideal_output);
        Ok(())
    }
    #[test]
    fn address_malformed_slashes() -> Result<(), AppError> {
        let healthy_prefix = "/avatar/parameters";
        let potential_addr = "/hr//test///";

        let ideal_output = "/avatar/parameters/hr/test";
        let test = format_address(healthy_prefix, potential_addr, "address_malformed_slashes")?;
        assert_eq!(test, ideal_output);
        Ok(())
    }

    #[test]
    fn both_malformed_slashes() -> Result<(), AppError> {
        let potential_prefix = "avatar//parameters///";
        let potential_addr = "//hr//test///";

        let ideal_output = "/avatar/parameters/hr/test";
        let prefix = format_prefix(potential_prefix);
        let test = format_address(&prefix?, potential_addr, "both_malformed_slashes")?;
        assert_eq!(test, ideal_output);
        Ok(())
    }

    #[test]
    #[should_panic(expected = "OscPrefix")]
    fn prefix_bad_character() {
        let potential_input = "/avatar/[]";

        format_prefix(potential_input).unwrap();
    }

    #[test]
    #[should_panic(expected = "address_bad_character")] // should echo name of bad param
    fn address_bad_character() {
        let healthy_prefix = "/avatar/parameters";
        let potential_addr = "/h*";

        format_address(healthy_prefix, potential_addr, "address_bad_character").unwrap();
    }
    #[test]
    #[should_panic(expected = "address_empty")]
    fn address_empty() {
        let healthy_prefix = "/avatar/parameters";
        let potential_addr = "";

        format_address(healthy_prefix, potential_addr, "address_empty").unwrap();
    }
    #[test]
    #[should_panic(expected = "address_just_slash")]
    fn address_just_slash() {
        let healthy_prefix = "/avatar/parameters";
        let potential_addr = "/";

        format_address(healthy_prefix, potential_addr, "address_just_slash").unwrap();
    }
}
