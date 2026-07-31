use std::collections::{BTreeMap, BTreeSet};

use super::{
    Base, CONFIG_VERSION, Config, ConfigError, FileMapping, FileOptions, Image, ImageFiles,
    PlatformValue,
};

pub(super) fn validate(config: Config) -> Result<Config, ConfigError> {
    if config.containerless != CONFIG_VERSION {
        return Err(ConfigError::Invalid(format!(
            "unsupported containerless version {}: expected {CONFIG_VERSION}",
            config.containerless
        )));
    }
    if config.images.is_empty() {
        return Err(ConfigError::Invalid(
            "config must define at least one image".to_owned(),
        ));
    }

    for (name, image) in &config.images {
        if image.squash && image.flatten {
            return Err(ConfigError::Invalid(format!(
                "image {name:?} cannot enable both squash and flatten"
            )));
        }
        for layer in &image.layers {
            if !config.layers.contains_key(layer) {
                return Err(ConfigError::Invalid(format!(
                    "image {name:?} references unknown layer {layer:?}"
                )));
            }
        }
        if let Base::Local { image: base } = &image.base
            && !config.images.contains_key(base)
        {
            return Err(ConfigError::Invalid(format!(
                "image {name:?} references unknown base image {base:?}"
            )));
        }
        validate_image_files(name, &image.files)?;
    }
    for (name, layer) in &config.layers {
        validate_files(name, &layer.files)?;
    }

    let mut complete = BTreeSet::new();
    for name in config.images.keys() {
        validate_base_cycle(name, &config.images, &mut BTreeSet::new(), &mut complete)?;
    }
    Ok(config)
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
        match file {
            FileMapping::Shorthand(mapping) => {
                let destination = mapping
                    .split_once(':')
                    .filter(|(source, destination)| !source.is_empty() && !destination.is_empty())
                    .map(|(_, destination)| destination)
                    .ok_or_else(|| {
                        ConfigError::Invalid(format!(
                            "invalid file mapping {mapping:?} in {owner:?}; expected SOURCE:DESTINATION"
                        ))
                    })?;
                validate_destination(owner, destination)?;
            }
            FileMapping::Detailed(options) => validate_file_options(owner, options)?,
        }
    }
    Ok(())
}

fn validate_image_files(image: &str, files: &ImageFiles) -> Result<(), ConfigError> {
    match files {
        ImageFiles::Single(options) => validate_file_options(image, options),
        ImageFiles::Multiple(files) => validate_files(image, files),
    }
}

fn validate_file_options(owner: &str, options: &FileOptions) -> Result<(), ConfigError> {
    if let PlatformValue::Platforms(values) = &options.source {
        if values.is_empty() {
            return Err(ConfigError::Invalid(format!(
                "file source platform map in {owner:?} cannot be empty"
            )));
        }
        if let Some(platform) = values
            .keys()
            .find(|platform| platform.as_str() != "default" && !platform.contains('/'))
        {
            return Err(ConfigError::Invalid(format!(
                "invalid OCI platform {platform:?} in {owner:?}"
            )));
        }
    }
    validate_destination(owner, &options.to)
}

fn validate_destination(owner: &str, destination: &str) -> Result<(), ConfigError> {
    if !destination.starts_with('/') {
        return Err(ConfigError::Invalid(format!(
            "file destination {destination:?} in {owner:?} must be absolute"
        )));
    }
    Ok(())
}
