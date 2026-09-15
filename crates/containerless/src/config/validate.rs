use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use super::{Base, Config, Error as ConfigError, FileMapping, Image, PlatformValue};

pub(super) fn validate(config: Config) -> Result<Config, ConfigError> {
    if config.images.is_empty() {
        return Err(ConfigError::Invalid(
            "config must define at least one image".to_owned(),
        ));
    }

    for (name, image) in &config.images {
        if let Base::Local { image: base } = &image.base
            && !config.images.contains_key(base)
        {
            return Err(ConfigError::Invalid(format!(
                "image {name:?} references unknown base image {base:?}"
            )));
        }
    }
    let mut complete = BTreeSet::new();
    for name in config.images.keys() {
        validate_base_cycle(name, &config.images, &mut BTreeSet::new(), &mut complete)?;
    }

    for (name, image) in &config.images {
        if image.squash && image.flatten {
            return Err(ConfigError::Invalid(format!(
                "image {name:?} cannot enable both squash and flatten"
            )));
        }
        validate_platforms(name, &image.platforms)?;
        let target_platforms = config.platforms_for_image(name).unwrap();
        for layer in &image.layers {
            if !config.layers.contains_key(layer) {
                return Err(ConfigError::Invalid(format!(
                    "image {name:?} references unknown layer {layer:?}"
                )));
            }
        }
        validate_files(name, &image.files)?;
        validate_chain_files(name, &config, &target_platforms)?;
    }
    for (name, layer) in &config.layers {
        validate_files(name, &layer.files)?;
    }

    Ok(config)
}

fn validate_chain_files(
    image_name: &str,
    config: &Config,
    platforms: &[String],
) -> Result<(), ConfigError> {
    let mut current = image_name;
    loop {
        let image = &config.images[current];
        if current != image_name && !image.platforms.is_empty() {
            for platform in platforms {
                if !image.platforms.contains(platform) {
                    return Err(ConfigError::Invalid(format!(
                        "base image {current:?} does not provide platform {platform:?} required by {image_name:?}"
                    )));
                }
            }
        }
        for layer in &image.layers {
            let files = &config.layers[layer].files;
            validate_files_for_platforms(image_name, files, platforms)?;
        }
        validate_files_for_platforms(image_name, &image.files, platforms)?;
        match &image.base {
            Base::Local { image: parent } => current = parent,
            Base::External(_) => return Ok(()),
        }
    }
}

fn validate_base_cycle(
    name: &str,
    images: &BTreeMap<String, Image>,
    visiting: &mut BTreeSet<String>,
    complete: &mut BTreeSet<String>,
) -> Result<(), ConfigError> {
    if complete.contains(name) {
        return Ok(());
    }
    if !visiting.insert(name.to_owned()) {
        return Err(ConfigError::Invalid(format!(
            "configured-image base cycle includes {name:?}"
        )));
    }
    if let Base::Local { image: base } = &images[name].base {
        validate_base_cycle(base, images, visiting, complete)?;
    }
    visiting.remove(name);
    complete.insert(name.to_owned());
    Ok(())
}

fn validate_files(owner: &str, files: &[FileMapping]) -> Result<(), ConfigError> {
    for file in files {
        validate_file_options(owner, file)?;
    }
    Ok(())
}

fn validate_platforms(image: &str, platforms: &[String]) -> Result<(), ConfigError> {
    let mut seen = BTreeSet::new();
    for platform in platforms {
        if containerless_core::Platform::from_str(platform).is_err() {
            return Err(ConfigError::Invalid(format!(
                "invalid OCI platform {platform:?} in image {image:?}"
            )));
        }
        if !seen.insert(platform) {
            return Err(ConfigError::Invalid(format!(
                "duplicate OCI platform {platform:?} in image {image:?}"
            )));
        }
    }
    Ok(())
}

fn validate_files_for_platforms(
    image: &str,
    files: &[FileMapping],
    platforms: &[String],
) -> Result<(), ConfigError> {
    for options in files {
        for platform in platforms {
            if options.source.for_platform(platform).is_none() {
                return Err(ConfigError::Invalid(format!(
                    "file source in image {image:?} has no value for platform {platform:?}"
                )));
            }
        }
    }
    Ok(())
}

fn validate_file_options(owner: &str, options: &FileMapping) -> Result<(), ConfigError> {
    if let PlatformValue::Platforms(values) = &options.source {
        if values.is_empty() {
            return Err(ConfigError::Invalid(format!(
                "file source platform map in {owner:?} cannot be empty"
            )));
        }
        if let Some(platform) = values.keys().find(|platform| {
            platform.as_str() != "default"
                && containerless_core::Platform::from_str(platform).is_err()
        }) {
            return Err(ConfigError::Invalid(format!(
                "invalid OCI platform {platform:?} in {owner:?}"
            )));
        }
    }
    if let Some(mode) = &options.mode
        && (mode.len() != 4
            || !mode.starts_with('0')
            || !mode.bytes().all(|digit| matches!(digit, b'0'..=b'7')))
    {
        return Err(ConfigError::Invalid(format!(
            "invalid file mode {mode:?} in {owner:?}; expected a quoted octal mode such as \"0755\""
        )));
    }
    if !options.to.starts_with('/') {
        return Err(ConfigError::Invalid(format!(
            "file destination {:?} in {owner:?} must be absolute",
            options.to
        )));
    }
    Ok(())
}
