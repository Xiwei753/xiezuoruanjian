//! Windows AppContainer 沙箱：为 Agent CLI 创建操作系统级文件访问隔离。
//!
//! 每个 Agent 在独立的 AppContainer profile 中运行，只能访问明确授权的
//! 目录和 capability。正文工作区、应用数据根不在可访问 ACL 中，Agent
//! 只能通过 MCP broker 代理读写。
//!
//! 参考：
//! - https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer
//! - https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows::Win32::Security::{
    DeriveAppContainerSidFromAppContainerName, SECURITY_CAPAB1_0, SID_AND_ATTRIBUTES,
    WELL_KNOWN_SID_TYPE,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, UpdateProcThreadAttribute, WaitForSingleObject,
    EXTENDED_STARTUPINFO_PRESENT, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, PROCESS_INFORMATION, STARTUPINFOEXW,
    STARTUPINFOW,
};

const APPCONTAINER_PREFIX: &str = "Sujian.Agent.";

/// AppContainer profile 的稳定标识。同一 Agent ID 复用同一 profile，
/// 保持 CLI 认证/配置在多次会话间持久。
pub struct AppContainerProfile {
    name: String,
    sid: Vec<u8>,
}

impl AppContainerProfile {
    /// 创建或复用 AppContainer profile。
    pub fn for_agent(agent_id: &str) -> Result<Self, String> {
        let name = format!("{APPCONTAINER_PREFIX}{agent_id}");
        let name_wide: Vec<u16> = OsStr::new(&name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut sid_buffer = vec![0u8; 256];
        let mut sid_len = sid_buffer.len() as u32;

        let result = unsafe {
            DeriveAppContainerSidFromAppContainerName(
                PCWSTR(name_wide.as_ptr()),
                sid_buffer.as_mut_ptr() as *mut _,
                &mut sid_len,
            )
        };

        if result.is_err() {
            sid_buffer.resize(sid_len as usize, 0);
            unsafe {
                DeriveAppContainerSidFromAppContainerName(
                    PCWSTR(name_wide.as_ptr()),
                    sid_buffer.as_mut_ptr() as *mut _,
                    &mut sid_len,
                )
                .map_err(|e| format!("DeriveAppContainerSidFromAppContainerName failed: {e}"))?;
            }
        }

        sid_buffer.truncate(sid_len as usize);

        Ok(Self {
            name,
            sid: sid_buffer,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn sid(&self) -> &[u8] {
        &self.sid
    }
}

/// 受限进程句柄。Drop 时自动关闭所有句柄。
pub struct SandboxProcess {
    pub handle: HANDLE,
    pub thread_handle: HANDLE,
    pub pid: u32,
    job_handle: HANDLE,
}

impl SandboxProcess {
    /// 等待进程退出并返回退出码。
    pub fn wait_exit(&self) -> Result<u32, String> {
        unsafe {
            let result = WaitForSingleObject(self.handle, u32::MAX);
            if result != WAIT_OBJECT_0 {
                return Err(format!("WaitForSingleObject failed: {result:?}"));
            }
            let mut exit_code: u32 = 0;
            GetExitCodeProcess(self.handle, &mut exit_code)
                .map_err(|e| format!("GetExitCodeProcess failed: {e}"))?;
            Ok(exit_code)
        }
    }
}

impl Drop for SandboxProcess {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.thread_handle);
            let _ = CloseHandle(self.handle);
            let _ = CloseHandle(self.job_handle);
        }
    }
}

/// 在 AppContainer 中启动受限进程。
///
/// - `program`: CLI 可执行文件完整路径
/// - `args`: 命令行参数
/// - `working_dir`: 工作目录（Agent 专属，在 AppContainer 可访问 ACL 中）
/// - `profile`: AppContainer profile
/// - `stdin_read`/`stdout_write`/`stderr_write`: 管道句柄（只继承这些）
/// - `extra_readable_dirs`: AppContainer 需要只读访问的目录
/// - `extra_writable_dirs`: AppContainer 需要读写访问的目录
pub fn spawn_in_appcontainer(
    program: &str,
    args: &[String],
    working_dir: &Path,
    profile: &AppContainerProfile,
    stdin_read: HANDLE,
    stdout_write: HANDLE,
    stderr_write: HANDLE,
    extra_readable_dirs: &[PathBuf],
    extra_writable_dirs: &[PathBuf],
) -> Result<SandboxProcess, String> {
    let capabilities = build_security_capabilities(profile)?;

    let mut startup_info = STARTUPINFOEXW::default();
    startup_info.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup_info.StartupInfo.dwFlags |= 0x00000100; // STARTF_USESTDHANDLES
    startup_info.StartupInfo.hStdInput = stdin_read;
    startup_info.StartupInfo.hStdOutput = stdout_write;
    startup_info.StartupInfo.hStdError = stderr_write;

    let attribute_count = 2u32;
    let mut attr_list_size: usize = 0;
    unsafe {
        InitializeProcThreadAttributeList(
            ptr::null_mut(),
            attribute_count,
            0,
            &mut attr_list_size,
        )
    };
    let mut attr_list_buffer = vec![0u8; attr_list_size];
    startup_info.lpAttributeList = attr_list_buffer.as_mut_ptr() as *mut _;

    unsafe {
        InitializeProcThreadAttributeList(
            startup_info.lpAttributeList,
            attribute_count,
            0,
            &mut attr_list_size,
        )
        .map_err(|e| format!("InitializeProcThreadAttributeList failed: {e}"))?;
    }

    unsafe {
        UpdateProcThreadAttribute(
            startup_info.lpAttributeList,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            &capabilities as *const _ as *const _,
            std::mem::size_of::<SECURITY_CAPAB1_0>(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
        .map_err(|e| format!("UpdateProcThreadAttribute(SECURITY_CAPABILITIES) failed: {e}"))?;
    }

    let mut inherit_handles = [stdin_read, stdout_write, stderr_write];
    unsafe {
        UpdateProcThreadAttribute(
            startup_info.lpAttributeList,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            inherit_handles.as_mut_ptr() as *const _,
            std::mem::size_of::<[HANDLE; 3]>(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
        .map_err(|e| format!("UpdateProcThreadAttribute(HANDLE_LIST) failed: {e}"))?;
    }

    let mut cmd_line = format!("\"{program}\"");
    for arg in args {
        cmd_line.push(' ');
        if arg.contains(' ') {
            cmd_line.push('"');
            cmd_line.push_str(arg);
            cmd_line.push('"');
        } else {
            cmd_line.push_str(arg);
        }
    }
    let mut cmd_line_wide: Vec<u16> = OsStr::new(&cmd_line)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let working_dir_wide: Vec<u16> = OsStr::new(working_dir)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let job_handle = unsafe { CreateJobObjectW(ptr::null(), PCWSTR(ptr::null())) };
    if job_handle == INVALID_HANDLE_VALUE {
        unsafe { DeleteProcThreadAttributeList(startup_info.lpAttributeList) };
        return Err("CreateJobObjectW failed".to_string());
    }
    let mut job_info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    job_info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            job_handle,
            JobObjectExtendedLimitInformation,
            &job_info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };

    let mut process_info = PROCESS_INFORMATION::default();
    let result = unsafe {
        CreateProcessW(
            PCWSTR(ptr::null()),
            cmd_line_wide.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            true,
            EXTENDED_STARTUPINFO_PRESENT,
            ptr::null(),
            PCWSTR(working_dir_wide.as_ptr()),
            &mut startup_info.StartupInfo as *mut _ as *mut STARTUPINFOW,
            &mut process_info,
        )
    };

    unsafe { DeleteProcThreadAttributeList(startup_info.lpAttributeList) };

    if result.is_err() {
        unsafe { let _ = CloseHandle(job_handle); }
        return Err(format!("CreateProcessW failed: {:?}", result.unwrap_err()));
    }

    unsafe {
        let _ = AssignProcessToJobObject(job_handle, process_info.hProcess);
    }

    Ok(SandboxProcess {
        handle: process_info.hProcess,
        thread_handle: process_info.hThread,
        pid: process_info.dwProcessId,
        job_handle,
    })
}

fn build_security_capabilities(profile: &AppContainerProfile) -> Result<SECURITY_CAPAB1_0, String> {
    let sid_ptr = profile.sid.as_ptr() as *const _;

    let internet_client_sid =
        create_well_known_sid(WELL_KNOWN_SID_TYPE::WinCapabilityInternetClientSid)?;
    let private_network_sid =
        create_well_known_sid(WELL_KNOWN_SID_TYPE::WinCapabilityPrivateNetworkClientServerSid)?;

    let mut capabilities = vec![
        SID_AND_ATTRIBUTES {
            Sid: internet_client_sid.as_ptr() as *mut _,
            Attributes: 0,
        },
        SID_AND_ATTRIBUTES {
            Sid: private_network_sid.as_ptr() as *mut _,
            Attributes: 0,
        },
    ];

    Ok(SECURITY_CAPAB1_0 {
        AppContainerSid: sid_ptr as *mut _,
        Capabilities: capabilities.as_mut_ptr(),
        CapabilityCount: capabilities.len() as u32,
        Reserved: 0,
    })
}

fn create_well_known_sid(sid_type: WELL_KNOWN_SID_TYPE) -> Result<Vec<u8>, String> {
    let mut sid_buffer = vec![0u8; 256];
    let mut sid_len = sid_buffer.len() as u32;
    unsafe {
        windows::Win32::Security::CreateWellKnownSid(
            sid_type,
            ptr::null(),
            sid_buffer.as_mut_ptr() as *mut _,
            &mut sid_len,
        )
        .map_err(|e| format!("CreateWellKnownSid failed: {e}"))?;
    }
    sid_buffer.truncate(sid_len as usize);
    Ok(sid_buffer)
}

/// 为 AppContainer profile 设置目录 ACL。
///
/// `readable_dirs` 和 `writable_dirs` 中的目录会被添加到 AppContainer
/// 的可访问 ACL 中。不在这些列表中的目录（如正文工作区）对 Agent
/// 进程不可见。
pub fn grant_directory_access(
    profile: &AppContainerProfile,
    readable_dirs: &[PathBuf],
    writable_dirs: &[PathBuf],
) -> Result<(), String> {
    use windows::Win32::Security::Authorization::{
        BuildExplicitAccessWithNameW, SetEntriesInAclW, EXPLICIT_ACCESS_W, SET_ACCESS,
        TRUSTEE_W, TRUSTEE_IS_GROUP, TRUSTEE_IS_SID,
    };
    use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION};
    use windows::Win32::Storage::FileSystem::SetNamedSecurityInfoW;

    let sid_ptr = profile.sid.as_ptr() as *const _;

    for dir in readable_dirs.iter().chain(writable_dirs.iter()) {
        if !dir.exists() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("创建目录 {} 失败: {e}", dir.display()))?;
        }

        let dir_wide: Vec<u16> = OsStr::new(dir)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let access_mode = if writable_dirs.contains(dir) {
            0x1F01FFu32 // GENERIC_ALL
        } else {
            0x120089u32 // GENERIC_READ | GENERIC_EXECUTE
        };

        let mut explicit_access = EXPLICIT_ACCESS_W {
            grfAccessPermissions: access_mode,
            grfAccessMode: SET_ACCESS,
            grfInheritance: 0x03, // CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE
            Trustee: TRUSTEE_W {
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_GROUP,
                ptstrName: sid_ptr as *mut _,
                ..Default::default()
            },
        };

        let mut new_acl: windows::Win32::Security::PACL = ptr::null_mut();

        unsafe {
            let result = SetEntriesInAclW(1, &explicit_access, ptr::null(), &mut new_acl);
            if result != 0 {
                return Err(format!("SetEntriesInAclW failed: error {result}"));
            }

            let result = SetNamedSecurityInfoW(
                PCWSTR(dir_wide.as_ptr()),
                windows::Win32::Storage::FileSystem::SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                Some(new_acl),
                ptr::null_mut(),
            );
            let _ = windows::Win32::Foundation::LocalFree(new_acl as *mut _);

            if result.is_err() {
                return Err(format!(
                    "SetNamedSecurityInfoW failed for {}: {:?}",
                    dir.display(),
                    result.unwrap_err()
                ));
            }
        }
    }

    Ok(())
}
