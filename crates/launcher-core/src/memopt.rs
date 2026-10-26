//! 内存优化（Windows）：启动游戏前整理系统内存，让游戏拿到更多可用的物理内存。
//!
//! 思路与 PCL 相同，使用系统公开的内存管理接口：
//! - 完整优化（需要管理员权限）：取得 SeProfileSingleProcessPrivilege / SeIncreaseQuotaPrivilege 后，
//!   通过 `NtSetSystemInformation(SystemMemoryListInformation)` 清空所有进程的工作集、
//!   写回已修改页、清空备用列表，并清空系统文件缓存；
//! - 普通优化（无需管理员权限）：逐个清空当前用户能访问的进程的工作集。
//!
//! 被换出的内存会在程序需要时自动换回，不会丢失数据；优化后的一小段时间里其他程序可能略有卡顿。

use serde::{Deserialize, Serialize};

use crate::system::{memory, MemoryInfo};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeResult {
    pub before: MemoryInfo,
    pub after: MemoryInfo,
    /// 是否以完整模式（管理员权限）运行
    pub full: bool,
}

impl OptimizeResult {
    /// 可用内存增加了多少（MB）。
    pub fn freed_mb(&self) -> i64 {
        self.after.available_mb as i64 - self.before.available_mb as i64
    }
}

/// 当前系统是否支持内存优化。
pub fn supported() -> bool {
    cfg!(windows)
}

/// 整理内存（阻塞调用，通常耗时 1～5 秒）。
pub fn optimize() -> OptimizeResult {
    let before = memory();
    let full = imp::optimize();
    // 给系统一点时间更新统计
    std::thread::sleep(std::time::Duration::from_millis(300));
    OptimizeResult { before, after: memory(), full }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_NOT_ALL_ASSIGNED, HANDLE, LUID};
    use windows_sys::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES,
        TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::ProcessStatus::{K32EmptyWorkingSet, K32EnumProcesses};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SET_QUOTA,
    };

    #[link(name = "ntdll", kind = "raw-dylib")]
    extern "system" {
        fn NtSetSystemInformation(class: i32, info: *mut c_void, len: u32) -> i32;
    }

    const SYSTEM_FILE_CACHE_INFORMATION: i32 = 21;
    const SYSTEM_MEMORY_LIST_INFORMATION: i32 = 80;
    const MEMORY_EMPTY_WORKING_SETS: u32 = 2;
    const MEMORY_FLUSH_MODIFIED_LIST: u32 = 3;
    const MEMORY_PURGE_STANDBY_LIST: u32 = 4;
    const MEMORY_PURGE_LOW_PRIORITY_STANDBY_LIST: u32 = 5;

    #[repr(C)]
    struct SystemFileCacheInformation {
        current_size: usize,
        peak_size: usize,
        page_fault_count: u32,
        minimum_working_set: usize,
        maximum_working_set: usize,
        current_size_including_transition_in_pages: usize,
        peak_size_including_transition_in_pages: usize,
        transition_re_purpose_count: u32,
        flags: u32,
    }

    /// 为当前进程启用一项特权；没有该特权（未以管理员身份运行）时返回 false。
    fn enable_privilege(name: &str) -> bool {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: 所有指针都指向本函数内有效的局部变量；句柄在使用后关闭
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut luid = LUID { LowPart: 0, HighPart: 0 };
            let ok = LookupPrivilegeValueW(std::ptr::null(), wide.as_ptr(), &mut luid) != 0 && {
                let tp = TOKEN_PRIVILEGES {
                    PrivilegeCount: 1,
                    Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }],
                };
                AdjustTokenPrivileges(token, 0, &tp, 0, std::ptr::null_mut(), std::ptr::null_mut()) != 0
                    && GetLastError() != ERROR_NOT_ALL_ASSIGNED
            };
            CloseHandle(token);
            ok
        }
    }

    fn memory_list_command(command: u32) -> bool {
        let mut c = command;
        // SAFETY: 传入一个 u32 命令，长度与之匹配
        unsafe {
            NtSetSystemInformation(SYSTEM_MEMORY_LIST_INFORMATION, &mut c as *mut u32 as *mut c_void, 4) >= 0
        }
    }

    fn flush_file_cache() -> bool {
        let mut info = SystemFileCacheInformation {
            current_size: 0,
            peak_size: 0,
            page_fault_count: 0,
            minimum_working_set: usize::MAX,
            maximum_working_set: usize::MAX,
            current_size_including_transition_in_pages: 0,
            peak_size_including_transition_in_pages: 0,
            transition_re_purpose_count: 0,
            flags: 0,
        };
        // SAFETY: 结构体布局与 SYSTEM_FILECACHE_INFORMATION 一致，长度与之匹配
        unsafe {
            NtSetSystemInformation(
                SYSTEM_FILE_CACHE_INFORMATION,
                &mut info as *mut SystemFileCacheInformation as *mut c_void,
                std::mem::size_of::<SystemFileCacheInformation>() as u32,
            ) >= 0
        }
    }

    /// 逐个清空当前用户能访问的进程的工作集（不需要管理员权限）。
    fn empty_process_working_sets() {
        let mut pids = vec![0u32; 4096];
        let mut needed = 0u32;
        // SAFETY: 缓冲区大小按字节传入；返回的 needed 不超过缓冲区大小
        unsafe {
            if K32EnumProcesses(pids.as_mut_ptr(), (pids.len() * 4) as u32, &mut needed) == 0 {
                return;
            }
            let me = GetCurrentProcessId();
            for &pid in &pids[..(needed as usize / 4).min(pids.len())] {
                if pid == 0 || pid == me {
                    continue;
                }
                let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA, 0, pid);
                if !h.is_null() {
                    K32EmptyWorkingSet(h);
                    CloseHandle(h);
                }
            }
        }
    }

    /// 返回是否以完整模式运行。
    pub fn optimize() -> bool {
        let profile = enable_privilege("SeProfileSingleProcessPrivilege");
        let quota = enable_privilege("SeIncreaseQuotaPrivilege");
        if !profile {
            empty_process_working_sets();
            return false;
        }
        memory_list_command(MEMORY_EMPTY_WORKING_SETS);
        if quota {
            flush_file_cache();
        }
        memory_list_command(MEMORY_FLUSH_MODIFIED_LIST);
        memory_list_command(MEMORY_PURGE_LOW_PRIORITY_STANDBY_LIST);
        memory_list_command(MEMORY_PURGE_STANDBY_LIST);
        true
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn optimize() -> bool {
        false
    }
}
