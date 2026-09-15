use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use containerless_core::{
    FileMapping, Image, Layer, Platform, PlatformImage, Registry, RuntimeConfig,
};

use crate::cli::{BuildOptions, ImageSelection, KeyValue};
use crate::config::{self, Base, Config, Error as ConfigError};

pub(super) struct ImageBuilder {
    source: ImageSource,
    options: BuildOptions,
    registry: Registry,
}

enum ImageSource {
    Config {
        config: Config,
        directory: PathBuf,
        selection: ImageSelection,
    },
    Executable(PathBuf),
}

impl ImageBuilder {
    pub(super) fn load(
        file: Option<PathBuf>,
        selection: ImageSelection,
        options: BuildOptions,
    ) -> Result<Self, ConfigError> {
        let path = match file {
            Some(path) => path,
            None => [
                "containerless.toml",
                "containerless.json",
                "containerless.yaml",
                "containerless.yml",
            ]
            .into_iter()
            .map(PathBuf::from)
            .find(|path| path.exists())
            .ok_or_else(|| {
                ConfigError::Invalid(
                    "no config found; use --file or create containerless.toml".to_owned(),
                )
            })?,
        };
        let config = Config::load(&path)?;
        let directory = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_owned();
        Ok(Self {
            source: ImageSource::Config {
                config,
                directory,
                selection,
            },
            registry: Registry::new(options.plain_http.clone()),
            options,
        })
    }

    pub(super) fn from_source(source: PathBuf, options: BuildOptions) -> Self {
        Self {
            source: ImageSource::Executable(source),
            registry: Registry::new(options.plain_http.clone()),
            options,
        }
    }

    pub(super) fn options(&self) -> &BuildOptions {
        &self.options
    }

    pub(super) fn registry(&self) -> &Registry {
        &self.registry
    }

    pub(super) async fn resolve(&self) -> Result<containerless_core::ImageSet, String> {
        let (config, directory, selection) = match &self.source {
            ImageSource::Config {
                config,
                directory,
                selection,
            } => (config, directory, selection),
            ImageSource::Executable(source) => return self.resolve_source(source),
        };
        let names = selected_images(config, selection)?;
        let mut references = names
            .iter()
            .map(|name| (name.clone(), config.images[name].tags.clone()))
            .collect::<BTreeMap<_, _>>();
        for tag in &self.options.tags {
            if let Some((name, reference)) = tag.split_once('=') {
                let Some(image_references) = references.get_mut(name) else {
                    return Err(format!("tag references unselected image {name:?}"));
                };
                image_references.push(reference.to_owned());
            } else if names.len() == 1 {
                references.get_mut(&names[0]).unwrap().push(tag.clone());
            } else {
                return Err(format!(
                    "tag {tag:?} must use IMAGE=REFERENCE when multiple images are selected"
                ));
            }
        }
        for image_references in references.values_mut() {
            image_references.sort();
            image_references.dedup();
        }
        let mut reference_owners = BTreeMap::new();
        for (image, image_references) in &references {
            for reference in image_references {
                if let Some(previous) = reference_owners.insert(reference, image)
                    && previous != image
                {
                    return Err(format!(
                        "reference {reference:?} is assigned to both {previous:?} and {image:?}"
                    ));
                }
            }
        }

        let working_directory = std::env::current_dir().map_err(|error| error.to_string())?;
        let mut images = Vec::new();
        for name in names {
            images.push(
                self.resolve_image(
                    config,
                    directory,
                    &working_directory,
                    &name,
                    references.remove(&name).unwrap(),
                )
                .await?,
            );
        }
        Ok(containerless_core::ImageSet::new(images))
    }

    fn resolve_source(&self, source: &Path) -> Result<containerless_core::ImageSet, String> {
        let references = self
            .options
            .tags
            .iter()
            .map(|tag| {
                tag.split_once('=')
                    .map_or(tag.as_str(), |(_, reference)| reference)
            })
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if references.is_empty() {
            return Err("publish --from requires at least one --tag REFERENCE".to_owned());
        }
        let platforms = if self.options.platforms.is_empty() {
            vec![Platform::linux_for_host()]
        } else {
            self.options
                .platforms
                .iter()
                .map(|platform| Platform::from_str(platform).map_err(|error| error.to_string()))
                .collect::<Result<Vec<_>, _>>()?
        };
        let working_directory = std::env::current_dir().map_err(|error| error.to_string())?;
        let source = if source.is_absolute() {
            source.to_owned()
        } else {
            working_directory.join(source)
        };
        if !source.is_file() {
            return Err(format!(
                "publish --from source {} must be a file",
                source.display()
            ));
        }

        let mut files = vec![FileMapping {
            mode: Some(0o755),
            ..FileMapping::new(source, "/app")
        }];
        files.extend(self.options.copies.iter().map(|copy| {
            let source = if copy.source.is_absolute() {
                copy.source.clone()
            } else {
                working_directory.join(&copy.source)
            };
            FileMapping::new(source, copy.destination.clone())
        }));
        let mut labels = BTreeMap::new();
        apply_assignments(&mut labels, &self.options.labels);
        let mut annotations = BTreeMap::new();
        apply_assignments(&mut annotations, &self.options.annotations);

        Ok(containerless_core::ImageSet::new(vec![Image {
            name: "image".to_owned(),
            references,
            platforms: platforms
                .into_iter()
                .map(|platform| PlatformImage {
                    platform,
                    base: None,
                    layers: vec![Layer {
                        files: files.clone(),
                    }],
                    runtime_config: RuntimeConfig {
                        entrypoint: Some(vec!["/app".to_owned()]),
                        labels: labels.clone(),
                        ..RuntimeConfig::default()
                    },
                    annotations: annotations.clone(),
                    flatten: self.options.flatten,
                })
                .collect(),
        }]))
    }
}

fn selected_images(config: &Config, selection: &ImageSelection) -> Result<Vec<String>, String> {
    if selection.all {
        return Ok(config.images.keys().cloned().collect());
    }
    if !selection.targets.is_empty() {
        for name in &selection.targets {
            if !config.images.contains_key(name) {
                return Err(format!(
                    "unknown image {name:?}; available images: {}",
                    config.images.keys().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
        }
        let mut names = selection.targets.clone();
        names.sort();
        names.dedup();
        return Ok(names);
    }
    if config.images.len() == 1 {
        return Ok(config.images.keys().cloned().collect());
    }
    Err(format!(
        "select an image or use --all; available images: {}",
        config.images.keys().cloned().collect::<Vec<_>>().join(", ")
    ))
}

impl ImageBuilder {
    async fn resolve_image(
        &self,
        config: &Config,
        directory: &Path,
        working_directory: &Path,
        name: &str,
        references: Vec<String>,
    ) -> Result<Image, String> {
        let options = &self.options;
        let mut chain = vec![name];
        while let Base::Local { image: parent } = &config.images[*chain.last().unwrap()].base {
            chain.push(parent);
        }
        chain.reverse();

        let platform_names = if options.platforms.is_empty() {
            config
                .platforms_for_image(name)
                .ok_or_else(|| format!("cannot resolve platforms for image {name:?}"))?
        } else {
            options.platforms.clone()
        };

        let mut platforms = Vec::new();
        for platform_name in platform_names {
            let platform = Platform::from_str(&platform_name).map_err(|error| error.to_string())?;
            let root = &config.images[chain[0]];
            let base = match &root.base {
                Base::External(base) if base == "scratch" => None,
                Base::External(base) => Some(
                    self.registry
                        .pull(base, &platform)
                        .await
                        .map_err(|error| error.to_string())?,
                ),
                Base::Local { .. } => unreachable!("the root image has an external base"),
            };
            let mut layers = Vec::new();
            for image_name in &chain {
                let configured = &config.images[*image_name];
                let mut image_layers = Vec::new();
                for layer_name in &configured.layers {
                    let files =
                        resolve_files(&config.layers[layer_name].files, directory, &platform_name)?;
                    if !files.is_empty() {
                        image_layers.push(Layer { files });
                    }
                }
                let files = resolve_files(&configured.files, directory, &platform_name)?;
                if !files.is_empty() {
                    image_layers.push(Layer { files });
                }
                if configured.squash && image_layers.len() > 1 {
                    image_layers = vec![Layer {
                        files: image_layers
                            .into_iter()
                            .flat_map(|layer| layer.files)
                            .collect(),
                    }];
                }
                layers.extend(image_layers);
            }
            let copies = options.copies.iter().map(|copy| {
                let source = if copy.source.is_absolute() {
                    copy.source.clone()
                } else {
                    working_directory.join(&copy.source)
                };
                FileMapping::new(source, copy.destination.clone())
            });
            if options.squash {
                let mut files = layers
                    .into_iter()
                    .flat_map(|layer| layer.files)
                    .collect::<Vec<_>>();
                files.extend(copies);
                layers = (!files.is_empty())
                    .then_some(Layer { files })
                    .into_iter()
                    .collect();
            } else {
                let files = copies.collect::<Vec<_>>();
                if !files.is_empty() {
                    layers.push(Layer { files });
                }
            }

            let mut runtime_config = RuntimeConfig::default();
            let mut annotations = BTreeMap::new();
            for image_name in &chain {
                let configured = &config.images[*image_name];
                runtime_config.entrypoint =
                    configured.entrypoint.clone().or(runtime_config.entrypoint);
                runtime_config.command = configured.command.clone().or(runtime_config.command);
                runtime_config.user = configured.user.clone().or(runtime_config.user);
                runtime_config.workdir = configured.workdir.clone().or(runtime_config.workdir);
                runtime_config.stop_signal = configured
                    .stop_signal
                    .clone()
                    .or(runtime_config.stop_signal);
                runtime_config.expose = configured.expose.clone().or(runtime_config.expose);
                runtime_config.volumes = configured.volumes.clone().or(runtime_config.volumes);
                runtime_config.env.extend(configured.env.clone());
                runtime_config.labels.extend(configured.labels.clone());
                annotations.extend(configured.annotations.clone());
            }
            apply_assignments(&mut runtime_config.labels, &options.labels);
            apply_assignments(&mut annotations, &options.annotations);
            platforms.push(PlatformImage {
                platform,
                base,
                layers,
                runtime_config,
                annotations,
                flatten: options.flatten
                    || chain
                        .iter()
                        .any(|image_name| config.images[*image_name].flatten),
            });
        }
        Ok(Image {
            name: name.to_owned(),
            references,
            platforms,
        })
    }
}

fn resolve_files(
    files: &[config::FileMapping],
    directory: &Path,
    platform: &str,
) -> Result<Vec<FileMapping>, String> {
    files
        .iter()
        .map(|file| {
            let source = file
                .source
                .for_platform(platform)
                .ok_or_else(|| format!("file source has no value for platform {platform:?}"))?;
            if source.is_absolute()
                || source
                    .components()
                    .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
            {
                return Err(format!(
                    "config file source {:?} must be relative and cannot contain ..",
                    source
                ));
            }
            let (uid, gid) = parse_owner(file.owner.as_deref().unwrap_or("0"))?;
            let mode = file
                .mode
                .as_deref()
                .map(|mode| u32::from_str_radix(mode, 8).unwrap());
            Ok(FileMapping {
                source: directory.join(source),
                destination: file.to.clone(),
                include: file.include.clone(),
                exclude: file.exclude.clone(),
                mode,
                uid,
                gid,
                follow_symlinks: file.follow_symlinks,
                preserve_paths: file.preserve_paths,
            })
        })
        .collect()
}

fn parse_owner(owner: &str) -> Result<(u64, u64), String> {
    let (uid, gid) = owner.split_once(':').unwrap_or((owner, owner));
    let uid = uid
        .parse()
        .map_err(|_| format!("owner {owner:?} must contain numeric UID[:GID]"))?;
    let gid = gid
        .parse()
        .map_err(|_| format!("owner {owner:?} must contain numeric UID[:GID]"))?;
    Ok((uid, gid))
}

fn apply_assignments(values: &mut BTreeMap<String, String>, assignments: &[KeyValue]) {
    for assignment in assignments {
        values.insert(assignment.key.clone(), assignment.value.clone());
    }
}
