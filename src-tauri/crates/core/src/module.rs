//! 多模块管理抽象 —— Game Manage Center 的愿景：不只管理存档，还将管理修改器与 MOD。
//!
//! 预留方式：所有功能模块实现 [`GameModule`] 接口并向 [`modules`] 注册表登记。
//! 当前 saves（存档）与trainers（修改器）模块已实现；
//! mods 为占位登记。后续能力就绪即在对应模块上追加 capability 标签。

use serde::Serialize;

/// 一个功能模块对外暴露的元信息（UI 导航、能力探测都以此为准）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleInfo {
    /// 稳定标识：saves / trainers / mods
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    /// false = 用户功能未完整（UI 显示为禁用）
    pub implemented: bool,
    /// 能力标签，供 UI 与命令层探测
    pub capabilities: &'static [&'static str],
}

pub trait GameModule {
    fn info(&self) -> ModuleInfo;
}

/// 存档模块（完整实现；能力由 scanner/backup/restore 引擎承载）
pub struct SaveModule;

impl GameModule for SaveModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "saves",
            display_name: "存档",
            description: "自动扫描、备份与恢复游戏存档，支持多账号与多平台",
            implemented: true,
            capabilities: &["scan", "backup", "restore", "auto-backup"],
        }
    }
}

/// 修改器模块：搜索、下载安装、启动与更新已完整接入
pub struct TrainerModule;

impl GameModule for TrainerModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: "trainers",
            display_name: "修改器",
            description: "修改器的搜索、下载安装、启动与更新（风灵月影，多来源预留）",
            implemented: true,
            capabilities: &["search", "match-games", "download", "launch", "update"],
        }
    }
}

/// 预留占位模块
pub struct ReservedModule {
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
}

impl GameModule for ReservedModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            id: self.id,
            display_name: self.display_name,
            description: self.description,
            implemented: false,
            capabilities: &[],
        }
    }
}

/// 全部已注册模块（顺序即 UI 导航顺序）
pub fn modules() -> Vec<ModuleInfo> {
    vec![
        SaveModule.info(),
        TrainerModule.info(),
        ReservedModule {
            id: "mods",
            display_name: "MOD",
            description: "MOD 的安装、版本管理与冲突排查（规划中）",
        }
        .info(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_registry_invariants() {
        let mods = modules();
        // 存档模块必须存在、已实现且排在首位
        assert_eq!(mods[0].id, "saves");
        assert!(mods[0].implemented);
        assert!(mods[0].capabilities.contains(&"backup"));
        // 修改器模块已完整实现：检索/下载/启动/更新
        let trainers = mods.iter().find(|m| m.id == "trainers").expect("trainers 注册");
        assert!(trainers.implemented);
        assert!(trainers.capabilities.contains(&"search"));
        assert!(trainers.capabilities.contains(&"download"));
        assert!(trainers.capabilities.contains(&"launch"));
        // MOD 仍为占位
        let mods_mod = mods.iter().find(|m| m.id == "mods").expect("mods 注册");
        assert!(!mods_mod.implemented);
        assert!(mods_mod.capabilities.is_empty());
        // id 唯一
        let ids: Vec<_> = mods.iter().map(|m| m.id).collect();
        assert_eq!(ids.len(), std::collections::HashSet::<_>::from_iter(ids.iter().copied()).len());
    }
}
