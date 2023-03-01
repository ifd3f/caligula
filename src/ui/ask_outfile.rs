use std::path::PathBuf;

use inquire::Select;

use crate::device::BurnTarget;

pub fn ask_outfile() -> anyhow::Result<PathBuf> {
    let mut enumerator = udev::Enumerator::new()?;
    let devices = enumerator.scan_devices()?;

    let burn_targets = devices.filter_map(|d| BurnTarget::try_from(d).ok());

    //let devs = get_all_device_info(&devpaths)?;
    //println!("{:#?}", devs);

    let removables: Vec<BurnTarget> = burn_targets.filter(|t| t.removable == Some(true)).collect();

    if removables.is_empty() {
        eprintln!("No removable devices found!");
        Err(AskOutfileError::NoDevices)?;
    } else {
        let ans = Select::new("Select a device", removables);
        ans.prompt()?;
    }
    todo!()
}

#[derive(Debug, thiserror::Error)]
pub enum AskOutfileError {
    #[error("No removable devices found")]
    NoDevices,
}
