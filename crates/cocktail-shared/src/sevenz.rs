//! 7z 压缩包文件名识别：control 与 init 共用的纯函数。
//!
//! 实际的 7z 二进制内嵌与解压逻辑由 cocktail-init 子进程实现，
//! control 端通过 RPC 调用；此模块只沉淀跨进程复用的字符串校验。

/// 判断文件名是否属于 cocktail 支持的压缩包格式。
/// 大小写不敏感；用于上传/导入时的快速过滤。
pub fn is_supported_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".7z")
        || n.ends_with(".zip")
        || n.ends_with(".tar")
        || n.ends_with(".tar.gz")
        || n.ends_with(".tgz")
        || n.ends_with(".tar.xz")
        || n.ends_with(".tar.bz2")
        || n.ends_with(".gz")
        || n.ends_with(".xz")
        || n.ends_with(".bz2")
        || n.ends_with(".jar")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_common_pack_names() {
        assert!(is_supported_name("server.7z"));
        assert!(is_supported_name("pack.TAR.GZ"));
        assert!(!is_supported_name("notes.txt"));
    }
}
