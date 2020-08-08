use anyhow::{Context, Result};

use handlebars::Handlebars;

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use args::Options;
use config::{self, Files, Variables};
use filesystem::{self, FileCompareState};
use handlebars_helpers;

pub fn deploy(opt: Options) -> Result<()> {
    // Configuration
    info!("Loading configuration...");

    // Throughout this function I'll be referencing steps, those were described in issue #6

    // Step 1
    let (files, variables, helpers) =
        config::load_configuration(&opt.local_config, &opt.global_config)
            .context("Failed to get a configuration.")?;

    // Step 2-3
    let mut desired_symlinks = config::Files::new();
    let mut desired_templates = config::Files::new();

    // On Windows, you need developer mode to create symlinks.
    let symlinks_enabled = if filesystem::symlinks_enabled(&PathBuf::from("DOTTER_SYMLINK_TEST"))
        .context("Failed to check whether symlinks are enabled")?
    {
        true
    } else {
        error!(
            "No permission to create symbolic links.\n
On Windows, in order to create symbolic links you need to enable Developer Mode.\n
Proceeding by copying instead of symlinking."
        );
        false
    };

    for (source, target) in files {
        if symlinks_enabled
            && !is_template(&source).context(format!("check whether {:?} is a template", source))?
        {
            desired_symlinks.insert(source, target);
        } else {
            desired_templates.insert(source, target);
        }
    }

    // Step 4
    let config::Cache {
        symlinks: existing_symlinks,
        templates: existing_templates,
    } = config::load_cache(&opt.cache_file)?;

    let state = FileState::new(
        desired_symlinks,
        desired_templates,
        existing_symlinks.clone(),
        existing_templates.clone(),
        opt.cache_directory,
    );

    let mut actual_symlinks = existing_symlinks;
    let mut actual_templates = existing_templates;

    // Step 5+6
    let (deleted_symlinks, deleted_templates) = state.deleted_files();
    debug!("Deleted symlinks: {:?}", deleted_symlinks);
    debug!("Deleted templates: {:?}", deleted_templates);
    for deleted_symlink in deleted_symlinks {
        match delete_symlink(opt.act, &deleted_symlink, opt.force)
            .context(format!("Failed to delete symlink {}", deleted_symlink))?
        {
            DeleteAction::Deleted => {
                actual_symlinks.remove(&deleted_symlink.source);
            }
            DeleteAction::SkippedBecauseChanged => {
                error!("Symlink in target location {:?} does not point at source file {:?} - probably modified by user. Skipping.", &deleted_symlink.target, &deleted_symlink.source);
            }
            DeleteAction::DeletedBecauseMissing => {
                warn!(
                    "Symlink in target location {:?} does not exist. Removing from cache anyways.",
                    &deleted_symlink.target
                );
                actual_symlinks.remove(&deleted_symlink.source);
            }
        }
    }
    for deleted_template in deleted_templates {
        match delete_template(opt.act, &deleted_template, opt.force)
            .context(format!("Failed to delete template {}", deleted_template))?
        {
            DeleteAction::Deleted => {
                actual_templates.remove(&deleted_template.source);
            }
            DeleteAction::SkippedBecauseChanged => {
                error!("Template contents in target location {:?} does not equal cached contents - probably modified by user. Skipping.", &deleted_template.target);
            }
            DeleteAction::DeletedBecauseMissing => {
                warn!(
                    "Template in target location {:?} does not exist. Removing from cache anyways.",
                    &deleted_template.target
                );
                actual_templates.remove(&deleted_template.source);
            }
        }
    }

    // Prepare handlebars instance
    let mut handlebars = Handlebars::new();
    handlebars.register_escape_fn(|s| s.to_string()); // Disable html-escaping
    handlebars_helpers::register_rust_helpers(&mut handlebars);
    handlebars_helpers::register_script_helpers(&mut handlebars, helpers);

    // Step 7+8
    let (new_symlinks, new_templates) = state.new_files();
    debug!("New symlinks: {:?}", new_symlinks);
    debug!("New templates: {:?}", new_templates);
    for new_symlink in new_symlinks {
        match update_symlink(opt.act, &new_symlink, opt.force).context(format!("Failed to create new symlink {}", new_symlink))? {
            UpdateAction::UpdatedBecauseMissing => {actual_symlinks.insert(new_symlink.source, new_symlink.target);},
            UpdateAction::Updated => {
                warn!("Symlink in target location {:?} already existed. Adding to cache anyways.", new_symlink.target);
                actual_symlinks.insert(new_symlink.source, new_symlink.target);
            }
            UpdateAction::SkippedBecauseChanged => {
                error!("Symlink in target location {:?} not pointing to source. Skipping", new_symlink.target);
            }
        }
    }
    for new_template in new_templates {
        match update_template(opt.act, &new_template, &handlebars, &variables, opt.force)
            .context(format!("Failed to create new template {}", new_template))? {
            UpdateAction::UpdatedBecauseMissing => {actual_templates.insert(new_template.source, new_template.target);}
            UpdateAction::Updated => {
                warn!("File in target location {:?} already existed. Adding to cache anyways.", new_template.target);
                actual_templates.insert(new_template.source, new_template.target);
            }
            UpdateAction::SkippedBecauseChanged => {
                error!("Template contents in target location {:?} does not equal cached contents. Skipping.", &new_template.target);
           }
           }
    }

    // Step 9+10
    let (old_symlinks, old_templates) = state.old_files();
    debug!("Old symlinks: {:?}", old_symlinks);
    debug!("Old templates: {:?}", old_templates);
    for old_symlink in old_symlinks {
        match update_symlink(opt.act, &old_symlink, opt.force).context(format!("Failed to update symlink {}", old_symlink))? {
            UpdateAction::Updated => { }
            UpdateAction::UpdatedBecauseMissing => {
                warn!("Symlink in target location {:?} was missing. Creating it anyways.", old_symlink.target);
            },
            UpdateAction::SkippedBecauseChanged => {
                error!("Symlink in target location is {:?} not pointing to source. Skipping", old_symlink.target);
            }
        }
    }
    for old_template in old_templates {
        match update_template(opt.act, &old_template, &handlebars, &variables, opt.force)
            .context(format!("Failed to update template file {}", old_template))? {
            UpdateAction::Updated => { }
            UpdateAction::UpdatedBecauseMissing => {
                warn!("File in target location {:?} was missing. Creating it anyways.", old_template.target);
            }
            UpdateAction::SkippedBecauseChanged => {
                error!("Template contents in target location {:?} does not equal cached contents - probably changed by user. Skipping.", &old_template.target);
           }
           }
    }

    debug!("Actual symlinks: {:?}", actual_symlinks);
    debug!("Actual templates: {:?}", actual_templates);
    // Step 11
    if opt.act {
        config::save_cache(
            &opt.cache_file,
            config::Cache {
                symlinks: actual_symlinks,
                templates: actual_templates,
            },
        )?;
    }

    Ok(())
}

enum DeleteAction {
    Deleted,
    SkippedBecauseChanged,
    DeletedBecauseMissing,
}

fn delete_symlink(act: bool, symlink: &FileDescription, force: bool) -> Result<DeleteAction> {
    let mut comparison = filesystem::compare_symlink(&symlink.target, &symlink.source)
        .context("Failed to check whether symlink was changed")?;
    if force {
        comparison = comparison.forced();
    }

    match comparison {
        FileCompareState::Equal => {
            if act {
                fs::remove_file(&symlink.target).context("Failed to remove symlink")?;
                filesystem::delete_parents(&symlink.target, true)
                    .context("Failed to delete parents of symlink")?;
            }
            Ok(DeleteAction::Deleted)
        }
        FileCompareState::Changed => Ok(DeleteAction::SkippedBecauseChanged),
        FileCompareState::Missing => Ok(DeleteAction::DeletedBecauseMissing),
    }
}

fn delete_template(act: bool, template: &FileDescription, force: bool) -> Result<DeleteAction> {
    let mut comparison = filesystem::compare_template(&template.target, &template.cache)
        .context("Failed to check whether templated file was changed")?;
    if force {
        comparison = comparison.forced();
    }

    match comparison {
        FileCompareState::Equal => {
            if act {
                fs::remove_file(&template.target).context("Failed to remove target file")?;
                filesystem::delete_parents(&template.cache, false)
                    .context("Failed to delete parent directory in cache")?;
                filesystem::delete_parents(&template.target, true)
                    .context("Failed to delete target directory in filesystem")?;
            }
            Ok(DeleteAction::Deleted)
        }
        FileCompareState::Changed => Ok(DeleteAction::SkippedBecauseChanged),
        FileCompareState::Missing => Ok(DeleteAction::DeletedBecauseMissing),
    }
}

enum UpdateAction {
    Updated,
    SkippedBecauseChanged,
    UpdatedBecauseMissing,
}

fn update_symlink(act: bool, symlink: &FileDescription, force: bool) -> Result<UpdateAction> {
    let mut comparison = filesystem::compare_symlink(&symlink.target, &symlink.source)
        .context("Failed to check whether symlink was changed")?;
    if force && comparison == FileCompareState::Changed {
        fs::remove_file(&symlink.target)
            .context("Failed to delete existing target file (--force)")?;
        comparison = FileCompareState::Missing;
    }

    match comparison {
        FileCompareState::Equal => Ok(UpdateAction::Updated),
        FileCompareState::Changed => Ok(UpdateAction::SkippedBecauseChanged),
        FileCompareState::Missing => {
            if act {
                filesystem::make_symlink(&symlink.target, &symlink.source).context("Failed to create missing symlink")?;
            }
            Ok(UpdateAction::UpdatedBecauseMissing)
        }
    }
}

fn update_template(
    act: bool,
    template: &FileDescription,
    handlebars: &Handlebars,
    variables: &Variables,
    force: bool,
) -> Result<UpdateAction> {
    let mut comparison = filesystem::compare_template(&template.target, &template.cache)
        .context("Failed to check whether template was changed")?;
    if force {
        fs::remove_file(&template.target)
            .context("Failed to delete existing target file (--force)")?;
        comparison = FileCompareState::Missing;
    }

    match comparison {
        comparison @ FileCompareState::Equal | comparison @ FileCompareState::Missing => {
            if act {
                let rendered = handlebars
                    .render_template(
                        &fs::read_to_string(&template.source)
                            .context("Failed to read template source file")?,
                        variables,
                    )
                    .context("Failed to render template")?;

                fs::create_dir_all(&template.cache.parent().context("Failed to get parent of cache file")?)
                    .context("Failed to create parent for cache file")?;
                fs::write(&template.cache, rendered).context("Failed to write rendered template to cache")?;
                fs::copy(&template.cache, &template.target)
                    .context("Failed to copy template from cache to target")?;
            }
            Ok(match comparison {
                FileCompareState::Equal => UpdateAction::Updated,
                FileCompareState::Missing => UpdateAction::UpdatedBecauseMissing,
                _ => unreachable!(),
            })
        },
        FileCompareState::Changed => Ok(UpdateAction::SkippedBecauseChanged),
    }
}

fn is_template(source: &Path) -> Result<bool> {
    let mut file = File::open(source).context(format!("Failed to open file {:?}", source))?;
    let mut buf = String::new();
    if file.read_to_string(&mut buf).is_err() {
        warn!("File {:?} is not valid UTF-8 - not templating", source);
        Ok(false)
    } else {
        Ok(buf.contains("{{"))
    }
}

struct FileState {
    desired_symlinks: BTreeSet<FileDescription>,
    desired_templates: BTreeSet<FileDescription>,
    existing_symlinks: BTreeSet<FileDescription>,
    existing_templates: BTreeSet<FileDescription>,
}

#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord)]
struct FileDescription {
    source: PathBuf,
    target: PathBuf,
    cache: PathBuf,
}

impl std::fmt::Display for FileDescription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "{:?} -> {:?}", self.source, self.target)
    }
}

impl FileState {
    fn new(
        desired_symlinks: Files,
        desired_templates: Files,
        existing_symlinks: Files,
        existing_templates: Files,
        cache_dir: PathBuf,
    ) -> FileState {
        FileState {
            desired_symlinks: Self::files_to_set(desired_symlinks, &cache_dir),
            desired_templates: Self::files_to_set(desired_templates, &cache_dir),
            existing_symlinks: Self::files_to_set(existing_symlinks, &cache_dir),
            existing_templates: Self::files_to_set(existing_templates, &cache_dir),
        }
    }

    fn files_to_set(files: Files, cache_dir: &Path) -> BTreeSet<FileDescription> {
        files
            .into_iter()
            .map(|(source, target)| FileDescription {
                source: source.clone(),
                target,
                cache: cache_dir.join(source),
            })
            .collect()
    }

    fn deleted_files(&self) -> (Vec<FileDescription>, Vec<FileDescription>) {
        (
            self.existing_symlinks
                .difference(&self.desired_symlinks)
                .cloned()
                .collect(),
            self.existing_templates
                .difference(&self.desired_templates)
                .cloned()
                .collect(),
        )
    }
    fn new_files(&self) -> (Vec<FileDescription>, Vec<FileDescription>) {
        (
            self.desired_symlinks
                .difference(&self.existing_symlinks)
                .cloned()
                .collect(),
            self.desired_templates
                .difference(&self.existing_templates)
                .cloned()
                .collect(),
        )
    }
    fn old_files(&self) -> (Vec<FileDescription>, Vec<FileDescription>) {
        (
            self.desired_symlinks
                .intersection(&self.existing_symlinks)
                .cloned()
                .collect(),
            self.existing_templates
                .intersection(&self.existing_templates)
                .cloned()
                .collect(),
        )
    }
}

#[cfg(test)]
mod test {
    use super::{FileDescription, FileState, Files, PathBuf};

    #[test]
    fn test_file_state_symlinks_only() {
        // Testing symlinks only is enough for me because the logic should be the same
        let mut existing_symlinks = Files::new();
        existing_symlinks.insert("file1s".into(), "file1t".into()); // Same
        existing_symlinks.insert("file2s".into(), "file2t".into()); // Deleted
        existing_symlinks.insert("file3s".into(), "file3t".into()); // Target change

        let mut desired_symlinks = Files::new();
        desired_symlinks.insert("file1s".into(), "file1t".into()); // Same
        desired_symlinks.insert("file3s".into(), "file0t".into()); // Target change
        desired_symlinks.insert("file5s".into(), "file5t".into()); // New

        let state = FileState::new(
            desired_symlinks,
            Files::new(),
            existing_symlinks,
            Files::new(),
            "cache".into(),
        );

        assert_eq!(
            state.deleted_files(),
            (
                vec![
                    FileDescription {
                        source: "file2s".into(),
                        target: "file2t".into(),
                        cache: PathBuf::from("cache").join("file2s"),
                    },
                    FileDescription {
                        source: "file3s".into(),
                        target: "file3t".into(),
                        cache: PathBuf::from("cache").join("file3s"),
                    }
                ],
                Vec::new()
            ),
            "deleted files correct"
        );
        assert_eq!(
            state.new_files(),
            (
                vec![
                    FileDescription {
                        source: "file3s".into(),
                        target: "file0t".into(),
                        cache: PathBuf::from("cache").join("file3s")
                    },
                    FileDescription {
                        source: "file5s".into(),
                        target: "file5t".into(),
                        cache: PathBuf::from("cache").join("file5s")
                    },
                ],
                Vec::new()
            ),
            "new files correct"
        );
        assert_eq!(
            state.old_files(),
            (
                vec![FileDescription {
                    source: "file1s".into(),
                    target: "file1t".into(),
                    cache: PathBuf::from("cache").join("file1s"),
                }],
                Vec::new()
            ),
            "old files correct"
        );
    }
}
