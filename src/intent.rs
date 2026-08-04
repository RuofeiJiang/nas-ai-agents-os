//! 自然语言意图解析 -> AAOS 动作
//!
//! MVP 阶段用基于规则的解析,覆盖常见中英文短语。
//! 普通意图映射到 [`crate::omv::OmvCall`](走 omv-rpc);
//! "应用配置"是元操作(取 dirty 模块 -> applyChanges),映射到 [`Action::ApplyChanges`]。
//! 之后可替换为 LLM-based 解析(返回 Action 即可,接口兼容)。

use crate::omv::OmvCall;
use serde_json::json;

/// 解析出的动作
#[derive(Debug, Clone)]
pub enum Action {
    /// 一次 OMV RPC 调用
    Call(OmvCall),
    /// 应用所有 dirty 配置(OMV 的 Apply)
    ApplyChanges,
}

/// 解析自然语言输入为动作。无法识别返回 None。
pub fn parse(input: &str) -> Option<Action> {
    let s = input.trim().to_lowercase();

    // ---- 元操作:应用配置 ----
    if matches_any(&s, &["应用配置", "应用变更", "apply", "应用一下", "部署配置"]) {
        return Some(Action::ApplyChanges);
    }

    // ---- 只读:磁盘 ----
    if matches_any(&s, &["列出磁盘", "列磁盘", "list disk", "磁盘列表", "所有磁盘"]) {
        return Some(Action::Call(OmvCall::new(
            "DiskMgmt", "enumerateDevices", json!({}), "列出所有磁盘",
        )));
    }

    // ---- 只读:系统信息 ----
    if matches_any(&s, &["系统信息", "系统状态", "system info", "主机信息"]) {
        return Some(Action::Call(OmvCall::new(
            "System", "getInformation", json!({}), "查看系统信息",
        )));
    }

    // ---- 只读:文件系统 ----
    if matches_any(&s, &["列出文件系统", "文件系统列表", "list filesystem", "所有文件系统"]) {
        return Some(Action::Call(OmvCall::new(
            "FileSystemMgmt", "enumerateFilesystems", json!({}), "列出文件系统",
        )));
    }
    if matches_any(&s, &["文件系统候选", "fs候选", "fs candidate", "能建什么"]) {
        return Some(Action::Call(OmvCall::new(
            "FileSystemMgmt", "getCandidates", json!({}), "列出可建文件系统的磁盘",
        )));
    }
    if matches_any(&s, &["已挂载", "挂载列表", "mounted filesystem"]) {
        return Some(Action::Call(OmvCall::new(
            "FileSystemMgmt", "enumerateMountedFilesystems", json!({}), "列出已挂载文件系统",
        )));
    }

    // ---- 只读:共享文件夹 ----
    if matches_any(&s, &["列出共享", "共享列表", "list share", "所有共享", "shared folder"]) {
        return Some(Action::Call(OmvCall::new(
            "ShareMgmt", "enumerateSharedFolders", json!({}), "列出共享文件夹",
        )));
    }

    // ---- 只读:SMB 共享 ----
    if matches_any(&s, &["列出smb", "smb共享列表", "list smb", "samba共享", "smb list"]) {
        return Some(Action::Call(OmvCall::new(
            "SMB", "getShareList", json!({"start":0,"limit":50}), "列出 SMB 共享",
        )));
    }

    // ---- 只读:用户 ----
    if matches_any(&s, &["列出用户", "用户列表", "list user", "所有用户"]) {
        return Some(Action::Call(OmvCall::new(
            "UserMgmt", "enumerateUsers", json!({}), "列出用户",
        )));
    }

    // ---- 只读:网络接口 ----
    if matches_any(&s, &["网络接口", "网卡列表", "network interface", "list network"]) {
        return Some(Action::Call(OmvCall::new(
            "Network", "enumerateInterfaces", json!({}), "列出网络接口",
        )));
    }

    None
}

fn matches_any(s: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| s.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_list_disks() {
        match parse("列出磁盘").unwrap() {
            Action::Call(c) => {
                assert_eq!(c.service, "DiskMgmt");
                assert_eq!(c.method, "enumerateDevices");
            }
            _ => panic!("expected Call"),
        }
    }

    #[test]
    fn parse_apply_changes() {
        assert!(matches!(parse("应用配置").unwrap(), Action::ApplyChanges));
        assert!(matches!(parse("apply").unwrap(), Action::ApplyChanges));
    }

    #[test]
    fn parse_unknown() {
        assert!(parse("随便说点什么").is_none());
    }
}
