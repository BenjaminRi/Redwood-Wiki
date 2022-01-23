use serde::Deserialize;

use std::io::ErrorKind;
use std::net::IpAddr;
use std::path::PathBuf;

#[derive(Deserialize, Debug)]
pub struct Config {
	pub network: Network,
}

#[derive(Deserialize, Debug)]
pub struct Network {
	pub ip: IpAddr,
	pub port: u16,
}

pub fn parse_config() -> std::io::Result<Config> {
	let mut exe_path = std::env::current_exe()?.canonicalize()?;
	exe_path.pop();
	exe_path.push("wiki-config.toml");
	parse_config_internal(vec![PathBuf::from("wiki-config.toml"), exe_path])
}

pub fn parse_config_internal(files: Vec<PathBuf>) -> std::io::Result<Config> {
	log::debug!("Config files: {:?}", files);
	for file in files {
		let toml_content = std::fs::read_to_string(&file);
		match toml_content {
			Ok(toml_content) => {
				let config: Config = toml::from_str(&toml_content)?;
				log::info!("Config file: {:?}", file);
				log::info!("Config contents: {:?}", config);
				return Ok(config);
			}
			Err(e) => match e.kind() {
				ErrorKind::NotFound => {
					// If the file is not found, we continue looking
					// for existing config files in the vector
					continue;
				}
				_ => {
					// The file exists be we have I/O problems
					// Escalate error to caller
					return Err(e);
				}
			},
		}
	}
	Err(std::io::Error::new(
		ErrorKind::NotFound,
		"No files available.",
	))
}

/*
	let toml_content = r#"
		  [network]
		  ip = "192.168.0.1"
		  "#;
*/
