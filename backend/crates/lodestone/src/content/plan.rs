//! Install plans: what installing some projects takes. Required
//! dependencies are added (unless already installed), optional ones are
//! offered, and incompatibilities in either direction block the plan. The
//! UI shows the plan and applies its `install` list as is.

use std::collections::{HashSet, VecDeque};

use super::{ContentRecord, Providers, ResolvedVersion, Target};
use crate::error::{AppError, AppResult};
use crate::model::{
    ContentRef, ContentSource, DependencyKind, InstallPlan, InstalledVersion, PlanConflict, PlanItem, PlanSuggestion,
    VersionChannel,
};

/// The version to install when none was asked for: the newest release,
/// else the newest of anything.
pub fn pick(versions: Vec<ResolvedVersion>) -> Option<ResolvedVersion> {
    let release = versions.iter().position(|v| v.version.channel == VersionChannel::Release);
    match release {
        Some(index) => versions.into_iter().nth(index),
        None => versions.into_iter().next(),
    }
}

struct Pending {
    reference: ContentRef,
    /// The name of the project that needs this one; `None` when asked for.
    required_by: Option<String>,
}

pub async fn plan(
    providers: &Providers,
    target: &Target,
    installed: &[ContentRecord],
    requests: &[ContentRef],
) -> AppResult<InstallPlan> {
    let mut plan = InstallPlan { install: vec![], optional: vec![], conflicts: vec![] };
    let mut seen: HashSet<(ContentSource, String)> = HashSet::new();
    let mut queue: VecDeque<Pending> =
        requests.iter().map(|reference| Pending { reference: reference.clone(), required_by: None }).collect();
    let installed_project = |provider: ContentSource, project: &str| {
        installed.iter().find(|record| record.provider == provider && record.project_id.as_deref() == Some(project))
    };
    // (provider, project, wanted by) for optional dependencies.
    let mut optional: Vec<(ContentSource, String, String)> = vec![];

    while let Some(Pending { reference, required_by }) = queue.pop_front() {
        let provider = providers.for_target(reference.provider, target)?;
        let key = (reference.provider, reference.project_id.clone());
        if !seen.insert(key) {
            continue;
        }
        let existing = installed_project(reference.provider, &reference.project_id);
        // A dependency that's already installed is satisfied as is.
        if required_by.is_some() && existing.is_some() {
            continue;
        }
        let project = provider.project(&reference.project_id).await?;

        let chosen = match &reference.version_id {
            Some(version_id) => {
                let version = provider.version(version_id, target).await?;
                if version.version.project_id != project.id {
                    return Err(AppError::bad_request(format!(
                        "Version {version_id} doesn't belong to {}.",
                        project.name
                    )));
                }
                if version.version.compatible {
                    Some(version)
                } else if required_by.is_some() {
                    // A pinned dependency that doesn't fit: take the best that does.
                    pick(provider.versions(&project.id, target).await?)
                } else {
                    plan.conflicts.push(PlanConflict {
                        name: project.name.clone(),
                        message: format!(
                            "{} {} isn't built for {}.",
                            project.name,
                            version.version.version_number,
                            target.describe()
                        ),
                    });
                    continue;
                }
            }
            None => pick(provider.versions(&project.id, target).await?),
        };
        let Some(chosen) = chosen else {
            plan.conflicts.push(PlanConflict {
                name: project.name.clone(),
                message: match &required_by {
                    Some(parent) => {
                        format!("{parent} needs {}, which has no version for {}.", project.name, target.describe())
                    }
                    None => format!("{} has no version for {}.", project.name, target.describe()),
                },
            });
            continue;
        };
        // Asking for the version that's installed is a no-op.
        if existing.is_some_and(|record| record.version_id.as_deref() == Some(chosen.version.id.as_str())) {
            continue;
        }

        // Installed entries that refuse to run beside this project.
        for record in installed {
            let refuses = record.dependencies.iter().any(|dep| {
                dep.kind == DependencyKind::Incompatible && dep.project_id.as_deref() == Some(project.id.as_str())
            });
            if refuses && record.enabled {
                plan.conflicts.push(PlanConflict {
                    name: project.name.clone(),
                    message: format!("{} doesn't work with {}, which is installed.", record.name, project.name),
                });
            }
        }

        for dep in &chosen.version.dependencies {
            let dep_project = match (&dep.project_id, &dep.version_id) {
                (Some(project), _) => project.clone(),
                (None, Some(version)) => match provider.version(version, target).await {
                    Ok(found) => found.version.project_id,
                    Err(_) => continue,
                },
                (None, None) => continue,
            };
            match dep.kind {
                DependencyKind::Required => queue.push_back(Pending {
                    reference: ContentRef {
                        provider: reference.provider,
                        project_id: dep_project,
                        version_id: dep.version_id.clone(),
                    },
                    required_by: Some(project.name.clone()),
                }),
                DependencyKind::Optional => {
                    if installed_project(reference.provider, &dep_project).is_none() {
                        optional.push((reference.provider, dep_project, project.name.clone()));
                    }
                }
                DependencyKind::Incompatible => {
                    let clash = installed_project(reference.provider, &dep_project).filter(|record| record.enabled);
                    let clash_name = match clash {
                        Some(record) => Some(record.name.clone()),
                        None if requests.iter().any(|r| r.project_id == dep_project) => {
                            Some(provider.project(&dep_project).await.map(|p| p.name).unwrap_or(dep_project.clone()))
                        }
                        None => None,
                    };
                    if let Some(other) = clash_name {
                        plan.conflicts.push(PlanConflict {
                            name: project.name.clone(),
                            message: format!("{} doesn't work with {other}.", project.name),
                        });
                    }
                }
                DependencyKind::Embedded => {}
            }
        }

        plan.install.push(PlanItem {
            provider: reference.provider,
            project_id: project.id.clone(),
            version_id: chosen.version.id.clone(),
            name: project.name.clone(),
            version_number: chosen.version.version_number.clone(),
            filename: chosen.version.filename.clone(),
            size_bytes: chosen.version.size_bytes,
            icon_url: project.icon_url.clone(),
            required_by,
            replaces: existing.map(|record| InstalledVersion {
                entry_id: record.id.clone(),
                version_name: record.version_name.clone(),
            }),
            manual_download: chosen.file.manual_url.clone().filter(|_| chosen.file.is_manual()),
        });
    }

    for (provider_id, project_id, wanted_by) in optional {
        let planned = plan.install.iter().any(|item| item.project_id == project_id);
        let offered = plan.optional.iter().any(|item| item.project_id == project_id);
        if planned || offered {
            continue;
        }
        let provider = providers.get(provider_id)?;
        let Ok(project) = provider.project(&project_id).await else { continue };
        let fits = provider.versions(&project.id, target).await.map(|v| !v.is_empty()).unwrap_or(false);
        if fits {
            plan.optional.push(PlanSuggestion {
                provider: provider_id,
                project_id: project.id,
                name: project.name,
                suggested_by: wanted_by,
            });
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::content::fake::FakeContent;
    use crate::content::ContentProvider;
    use crate::content::ProjectInfo;
    use crate::model::{ModKind, ServerSoftwareId};

    fn providers() -> Providers {
        Providers::new(vec![Arc::new(FakeContent::default())])
    }

    fn target(software: ServerSoftwareId, version: &str) -> Target {
        Target {
            software,
            kind: if software == ServerSoftwareId::Paper { ModKind::Plugin } else { ModKind::Mod },
            loaders: Target::loaders_for(software),
            game_version: version.into(),
        }
    }

    fn ask(project: &str) -> ContentRef {
        ContentRef { provider: ContentSource::Modrinth, project_id: project.into(), version_id: None }
    }

    async fn installed(project: &str, target: &Target) -> ContentRecord {
        let fake = FakeContent::default();
        let info: ProjectInfo = fake.project(project).await.unwrap();
        let version = pick(fake.versions(project, target).await.unwrap()).unwrap();
        ContentRecord::from_version(format!("entry-{project}"), &info, &version, target.kind, false, "now".into())
    }

    #[tokio::test]
    async fn adds_required_dependencies_transitively() {
        let target = target(ServerSoftwareId::Fabric, "1.21.4");
        let plan = plan(&providers(), &target, &[], &[ask("fk-ledger")]).await.unwrap();
        let names: Vec<_> = plan.install.iter().map(|i| (i.name.as_str(), i.required_by.as_deref())).collect();
        assert_eq!(
            names,
            [("Ledger", None), ("Fabric API", Some("Ledger")), ("Fabric Language Kotlin", Some("Ledger"))]
        );
        assert!(plan.conflicts.is_empty());
        assert_eq!(plan.install[1].version_number, "0.119.2+1.21.4", "the build for this Minecraft version");
    }

    #[tokio::test]
    async fn skips_dependencies_that_are_already_installed() {
        let target = target(ServerSoftwareId::Paper, "1.21.4");
        let worldedit = installed("fk-worldedit", &target).await;
        let plan = plan(&providers(), &target, &[worldedit], &[ask("fk-worldguard")]).await.unwrap();
        assert_eq!(plan.install.len(), 1);
        assert_eq!(plan.install[0].name, "WorldGuard");
    }

    #[tokio::test]
    async fn offers_optional_dependencies() {
        let target = target(ServerSoftwareId::Paper, "1.21.4");
        let plan = plan(&providers(), &target, &[], &[ask("fk-luckperms")]).await.unwrap();
        assert_eq!(plan.install.len(), 1);
        assert_eq!(plan.optional.len(), 1);
        assert_eq!((plan.optional[0].name.as_str(), plan.optional[0].suggested_by.as_str()), ("Vault", "LuckPerms"));
    }

    #[tokio::test]
    async fn incompatibilities_block_in_both_directions() {
        let target = target(ServerSoftwareId::Paper, "1.21.4");
        let essentials = installed("fk-essentialsx", &target).await;
        let plan1 = plan(&providers(), &target, &[essentials], &[ask("fk-homeslite")]).await.unwrap();
        assert_eq!(plan1.conflicts.len(), 1, "{:?}", plan1.conflicts);
        assert!(plan1.conflicts[0].message.contains("EssentialsX"));

        let homes = installed("fk-homeslite", &target).await;
        let plan2 = plan(&providers(), &target, &[homes], &[ask("fk-essentialsx")]).await.unwrap();
        assert_eq!(plan2.conflicts.len(), 1, "{:?}", plan2.conflicts);
    }

    #[tokio::test]
    async fn reports_projects_with_no_build_for_the_version() {
        let target = target(ServerSoftwareId::Paper, "1.21.4");
        let plan = plan(&providers(), &target, &[], &[ask("fk-coreprotect")]).await.unwrap();
        assert!(plan.install.is_empty());
        assert_eq!(plan.conflicts[0].message, "CoreProtect has no version for Paper 1.21.4.");
    }

    #[tokio::test]
    async fn an_update_replaces_the_installed_version() {
        let target = target(ServerSoftwareId::Paper, "1.21.4");
        let fake = FakeContent::default();
        let info = fake.project("fk-worldedit").await.unwrap();
        let old = fake.version("fk-worldedit-7.3.9", &target).await.unwrap();
        let record = ContentRecord::from_version("e1".into(), &info, &old, ModKind::Plugin, false, "now".into());
        let plan = plan(&providers(), &target, std::slice::from_ref(&record), &[ask("fk-worldedit")]).await.unwrap();
        assert_eq!(plan.install[0].version_number, "7.3.10");
        assert_eq!(plan.install[0].replaces.as_ref().unwrap().version_name, "7.3.9");

        // Already the newest: nothing to do.
        let current = installed("fk-worldedit", &target).await;
        let plan = super::plan(&providers(), &target, &[current], &[ask("fk-worldedit")]).await.unwrap();
        assert!(plan.install.is_empty());
    }
}
