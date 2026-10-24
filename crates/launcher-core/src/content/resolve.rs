//! 前置依赖解析：递归解析 required 依赖（处理循环依赖与去重），列出可选依赖与冲突。

use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;

use serde::Serialize;

use super::{ContentVersion, DepKind};
use crate::error::Result;

/// 依赖解析所需的数据源。
pub trait DepSource: Sync {
    /// 获取指定版本。
    fn version(&self, project_id: &str, version_id: &str) -> impl Future<Output = Result<ContentVersion>> + Send;
    /// 依赖只给了项目、没给版本时，选出兼容当前实例的最新版本。
    fn best_version(&self, project_id: &str) -> impl Future<Output = Result<Option<ContentVersion>>> + Send;
    fn title(&self, project_id: &str) -> impl Future<Output = Result<String>> + Send;
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub project_id: String,
    pub title: String,
    pub version: ContentVersion,
    /// 实例中已经安装了这个项目（会跳过）
    pub installed: bool,
    /// 由哪个项目引入（根项目为 None）
    pub required_by: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingDep {
    pub project_id: String,
    pub title: String,
    pub required_by: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// 要安装的项目：根项目在前，随后是递归解析出的必需前置
    pub items: Vec<PlanItem>,
    /// 可选依赖（不自动安装，由用户勾选）
    pub optional: Vec<PlanItem>,
    /// 已安装但与将要安装的内容冲突的项目
    pub incompatible: Vec<MissingDep>,
    /// 找不到兼容版本的必需前置
    pub missing: Vec<MissingDep>,
}

/// 从根版本出发解析依赖。`installed` 为实例中已安装项目的 id。
pub async fn resolve<S: DepSource>(src: &S, roots: Vec<ContentVersion>, installed: &HashSet<String>) -> Result<Plan> {
    let mut plan = Plan::default();
    let mut visited: HashSet<String> = HashSet::new();
    let mut titles: HashMap<String, String> = HashMap::new();
    let mut queue: VecDeque<(ContentVersion, Option<String>)> = VecDeque::new();

    for root in roots {
        if visited.insert(root.project_id.clone()) {
            queue.push_back((root, None));
        }
    }

    while let Some((version, required_by)) = queue.pop_front() {
        let title = title_of(src, &mut titles, &version.project_id).await;
        let deps = version.dependencies.clone();
        plan.items.push(PlanItem {
            project_id: version.project_id.clone(),
            title: title.clone(),
            installed: installed.contains(&version.project_id) && required_by.is_some(),
            version,
            required_by,
        });

        for dep in deps {
            // 只给了版本 id 的依赖，需要先查出所属项目
            let (project_id, pinned) = match (&dep.project_id, &dep.version_id) {
                (Some(p), v) => (p.clone(), v.clone()),
                (None, Some(v)) => match src.version("", v).await {
                    Ok(ver) => (ver.project_id.clone(), Some(v.clone())),
                    Err(_) => continue,
                },
                (None, None) => continue,
            };
            match dep.kind {
                DepKind::Embedded => {}
                DepKind::Incompatible => {
                    if installed.contains(&project_id) {
                        let dep_title = title_of(src, &mut titles, &project_id).await;
                        plan.incompatible.push(MissingDep {
                            project_id,
                            title: dep_title,
                            required_by: title.clone(),
                            reason: format!("与「{title}」不兼容"),
                        });
                    }
                }
                DepKind::Required => {
                    if !visited.insert(project_id.clone()) {
                        continue; // 已处理过（去重、避免循环依赖）
                    }
                    let dep_title = title_of(src, &mut titles, &project_id).await;
                    if installed.contains(&project_id) {
                        // 已安装的前置不再下载，也不再展开它的依赖
                        if let Some(v) = fetch(src, &project_id, pinned.as_deref()).await {
                            plan.items.push(PlanItem {
                                project_id,
                                title: dep_title,
                                version: v,
                                installed: true,
                                required_by: Some(title.clone()),
                            });
                        }
                        continue;
                    }
                    match fetch(src, &project_id, pinned.as_deref()).await {
                        Some(v) => queue.push_back((v, Some(title.clone()))),
                        None => plan.missing.push(MissingDep {
                            project_id,
                            title: dep_title,
                            required_by: title.clone(),
                            reason: "没有找到兼容当前游戏版本与加载器的版本".into(),
                        }),
                    }
                }
                DepKind::Optional => {
                    if visited.contains(&project_id)
                        || installed.contains(&project_id)
                        || plan.optional.iter().any(|o| o.project_id == project_id)
                    {
                        continue;
                    }
                    if let Some(v) = fetch(src, &project_id, pinned.as_deref()).await {
                        let dep_title = title_of(src, &mut titles, &project_id).await;
                        plan.optional.push(PlanItem {
                            project_id,
                            title: dep_title,
                            version: v,
                            installed: false,
                            required_by: Some(title.clone()),
                        });
                    }
                }
            }
        }
    }
    // 被选为必需前置的项目不再出现在可选列表
    let required: HashSet<String> = plan.items.iter().map(|i| i.project_id.clone()).collect();
    plan.optional.retain(|o| !required.contains(&o.project_id));
    Ok(plan)
}

async fn fetch<S: DepSource>(src: &S, project_id: &str, version_id: Option<&str>) -> Option<ContentVersion> {
    match version_id {
        Some(v) => src.version(project_id, v).await.ok(),
        None => src.best_version(project_id).await.ok().flatten(),
    }
}

async fn title_of<S: DepSource>(src: &S, cache: &mut HashMap<String, String>, project_id: &str) -> String {
    if let Some(t) = cache.get(project_id) {
        return t.clone();
    }
    let t = src.title(project_id).await.unwrap_or_else(|_| project_id.to_string());
    cache.insert(project_id.to_string(), t.clone());
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{Dependency, Platform};
    use crate::error::CoreError;

    struct Mock {
        versions: Vec<ContentVersion>,
    }

    fn ver(project: &str, id: &str, deps: &[(&str, Option<&str>, DepKind)]) -> ContentVersion {
        ContentVersion {
            platform: Platform::Modrinth,
            id: id.into(),
            project_id: project.into(),
            name: id.into(),
            version_number: id.into(),
            game_versions: vec!["1.20.1".into()],
            loaders: vec!["fabric".into()],
            release_type: "release".into(),
            published: "2024".into(),
            downloads: 0,
            files: vec![],
            dependencies: deps
                .iter()
                .map(|(p, v, k)| Dependency {
                    project_id: if p.is_empty() { None } else { Some(p.to_string()) },
                    version_id: v.map(str::to_string),
                    kind: *k,
                })
                .collect(),
        }
    }

    impl DepSource for Mock {
        fn version(&self, _project_id: &str, version_id: &str) -> impl Future<Output = Result<ContentVersion>> + Send {
            let r = self
                .versions
                .iter()
                .find(|v| v.id == version_id)
                .cloned()
                .ok_or_else(|| CoreError::InvalidData("no version".into()));
            async move { r }
        }
        fn best_version(&self, project_id: &str) -> impl Future<Output = Result<Option<ContentVersion>>> + Send {
            let r = self.versions.iter().find(|v| v.project_id == project_id).cloned();
            async move { Ok(r) }
        }
        fn title(&self, project_id: &str) -> impl Future<Output = Result<String>> + Send {
            let t = format!("《{project_id}》");
            async move { Ok(t) }
        }
    }

    fn ids(items: &[PlanItem]) -> Vec<&str> {
        items.iter().map(|i| i.project_id.as_str()).collect()
    }

    #[tokio::test]
    async fn recursive_required_with_cycle() {
        use DepKind::*;
        // extra → sodium → (fabric-api)；fabric-api → extra（循环）
        let m = Mock {
            versions: vec![
                ver("extra", "e1", &[("sodium", None, Required), ("fapi", None, Required), ("modmenu", None, Optional)]),
                ver("sodium", "s1", &[("fapi", None, Required)]),
                ver("fapi", "f1", &[("extra", None, Required)]),
                ver("modmenu", "mm1", &[]),
            ],
        };
        let root = m.versions[0].clone();
        let plan = resolve(&m, vec![root], &HashSet::new()).await.unwrap();
        assert_eq!(ids(&plan.items), vec!["extra", "sodium", "fapi"]);
        assert_eq!(plan.items[1].required_by.as_deref(), Some("《extra》"));
        assert_eq!(ids(&plan.optional), vec!["modmenu"]);
        assert!(plan.missing.is_empty());
    }

    #[tokio::test]
    async fn installed_missing_incompatible_and_pinned() {
        use DepKind::*;
        let m = Mock {
            versions: vec![
                ver("root", "r1", &[
                    ("lib", None, Required),
                    ("gone", None, Required),
                    ("", Some("pinned-v"), Required),
                    ("optifine", None, Incompatible),
                    ("other", None, Incompatible),
                ]),
                ver("lib", "l1", &[("deep", None, Required)]),
                ver("deep", "d1", &[]),
                ver("pinned", "pinned-v", &[]),
                ver("pinned", "pinned-latest", &[]),
            ],
        };
        let installed: HashSet<String> = ["lib".to_string(), "optifine".to_string()].into();
        let plan = resolve(&m, vec![m.versions[0].clone()], &installed).await.unwrap();
        // lib 已安装：列出但标记 installed，且不展开它的依赖 deep
        assert_eq!(ids(&plan.items), vec!["root", "lib", "pinned"]);
        assert!(plan.items[1].installed);
        assert!(!plan.items[0].installed);
        // 只给了版本 id 的依赖使用指定版本
        assert_eq!(plan.items[2].version.id, "pinned-v");
        assert_eq!(plan.missing.len(), 1);
        assert_eq!(plan.missing[0].project_id, "gone");
        // 只有已安装的冲突项才警告
        assert_eq!(plan.incompatible.len(), 1);
        assert_eq!(plan.incompatible[0].project_id, "optifine");
    }

    #[tokio::test]
    async fn multiple_roots_dedup() {
        use DepKind::*;
        let m = Mock {
            versions: vec![
                ver("a", "a1", &[("lib", None, Required), ("b", None, Optional)]),
                ver("b", "b1", &[("lib", None, Required)]),
                ver("lib", "l1", &[]),
            ],
        };
        let plan = resolve(&m, vec![m.versions[0].clone(), m.versions[1].clone()], &HashSet::new())
            .await
            .unwrap();
        assert_eq!(ids(&plan.items), vec!["a", "b", "lib"]);
        // b 已作为根安装，不再出现在可选列表
        assert!(plan.optional.is_empty());
    }
}
