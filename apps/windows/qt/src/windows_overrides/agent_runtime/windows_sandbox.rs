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

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, BOOL, HANDLE, HLOCAL, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows::Win32::Security::{
    FreeSid, CreateWellKnownSid, ACL, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
    SECURITY_CAPABILITIES, SID_AND_ATTRIBUTES, WELL_KNOWN_SID_TYPE,
    DACL_SECURITY_INFORMATION,
};
use windows::Win32::Security::Authorization::{
    GetNamedSecurityInfoW, SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W,
    SET_ACCESS, TRUSTEE_W, TRUSTEE_IS_GROUP, TRUSTEE_IS_SID, SE_FILE_OBJECT,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_SUSPENDED, EXTENDED_STARTUPINFO_PRESENT,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
    PROCESS_INFORMATION, STARTUPINFOEXW, STARTUPINFOW,
};

const APPCONTAINER_PREFIX: &str = "Sujian.Agent.";

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// 拥有 `CreateAppContainerProfile`/`DeriveAppContainerSidFromAppContainerName`
/// 返回的 PSID 的 RAII 包装器。Drop 时调用 `FreeSid`。
pub struct OwnedSid {
    sid: PSID,
}

impl OwnedSid {
    pub fn as_psid(&self) -> PSID {
        self.sid
    }

    pub fn as_ptr(&self) -> *mut core::ffi::c_void {
        self.sid.0
    }
}

impl Drop for OwnedSid {
    fn drop(&mut self) {
        if !self.sid.0.is_null() {
            unsafe { FreeSid(self.sid) };
        }
    }
}

/// AppContainer profile 的稳定标识。同一 Agent ID 复用同一 profile，
/// 保持 CLI 认证/配置在多次会话间持久。
pub struct AppContainerProfile {
    name: String,
    sid: OwnedSid,
}

impl AppContainerProfile {
    pub fn for_agent(agent_id: &str) -> Result<Self, String> {
        let name = format!("{APPCONTAINER_PREFIX}{agent_id}");
        let name_wide = wide(&name);

        let sid = match unsafe {
            CreateAppContainerProfile(
                PCWSTR(name_wide.as_ptr()),
                PCWSTR(name_wide.as_ptr()),
                PCWSTR(name_wide.as_ptr()),
                Some(&[]),
            )
        } {
            Ok(psid) => OwnedSid { sid: psid },
            Err(_) => {
                let psid = unsafe {
                    DeriveAppContainerSidFromAppContainerName(PCWSTR(name_wide.as_ptr()))
                }
                .map_err(|e| format!("DeriveAppContainerSidFromAppContainerName: {e}"))?;
                OwnedSid { sid: psid }
            }
        };

        Ok(Self { name, sid })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn sid(&self) -> &OwnedSid {
        &self.sid
    }
}

/// 持有 `CreateWellKnownSid` 输出缓冲区的 RAII 结构。
///
/// `CreateWellKnownSid` 往调用方提供的缓冲区写入，不能用 `FreeSid` 释放。
/// 缓冲区由 `Vec<u8>` 持有，`SID_AND_ATTRIBUTES` 中存指向它的裸指针。
pub struct WellKnownSid {
    buffer: Vec<u8>,
}

impl WellKnownSid {
    pub fn new(sid_type: WELL_KNOWN_SID_TYPE) -> Result<Self, String> {
        let mut buffer = vec![0u8; 256];
        let mut sid_len = buffer.len() as u32;
        unsafe {
            CreateWellKnownSid(
                sid_type,
                None,
                PSID(buffer.as_mut_ptr().cast()),
                &mut sid_len,
            )
            .map_err(|e| format!("CreateWellKnownSid: {e}"))?;
        }
        buffer.truncate(sid_len as usize);
        Ok(Self { buffer })
    }

    pub fn as_ptr(&self) -> *mut core::ffi::c_void {
        self.buffer.as_ptr() as *mut _
    }
}

/// 拥有 `SECURITY_CAPABILITIES` 所有数据的结构体。
///
/// 借用 `AppContainerProfile` 的 SID（不复制），Capability SID 由
/// `WellKnownSid` 持有（`Vec<u8>` 缓冲区，不用 `FreeSid`）。
/// 所有指针在 `Self` 存活期间有效，直至 `CreateProcessW` 返回。
pub struct SandboxCapabilities<'a> {
    profile: &'a AppContainerProfile,
    internet_client_sid: WellKnownSid,
    private_network_sid: WellKnownSid,
    capabilities: Vec<SID_AND_ATTRIBUTES>,
}

impl<'a> SandboxCapabilities<'a> {
    pub fn new(profile: &'a AppContainerProfile) -> Result<Self, String> {
        let internet_client_sid =
            WellKnownSid::new(WELL_KNOWN_SID_TYPE::WinCapabilityInternetClientSid)?;
        let private_network_sid = WellKnownSid::new(
            WELL_KNOWN_SID_TYPE::WinCapabilityPrivateNetworkClientServerSid,
        )?;

        let capabilities = vec![
            SID_AND_ATTRIBUTES {
                Sid: internet_client_sid.as_ptr(),
                Attributes: 0,
            },
            SID_AND_ATTRIBUTES {
                Sid: private_network_sid.as_ptr(),
                Attributes: 0,
            },
        ];

        Ok(Self {
            profile,
            internet_client_sid,
            private_network_sid,
            capabilities,
        })
    }

    pub fn as_security_capabilities(&self) -> SECURITY_CAPABILITIES {
        SECURITY_CAPABILITIES {
            AppContainerSid: self.profile.sid.as_psid(),
            Capabilities: self.capabilities.as_ptr() as *mut _,
            CapabilityCount: self.capabilities.len() as u32,
            Reserved: 0,
        }
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
    pub fn wait_exit(&self) -> Result<u32, String> {
        unsafe {
            let result = WaitForSingleObject(self.handle, u32::MAX);
            if result != WAIT_OBJECT_0 {
                return Err(format!("WaitForSingleObject: {result:?}"));
            }
            let mut exit_code: u32 = 0;
            GetExitCodeProcess(self.handle, &mut exit_code)
                .map_err(|e| format!("GetExitCodeProcess: {e}"))?;
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

/// 为 AppContainer profile 设置目录 ACL。
///
/// 使用 `GetNamedSecurityInfoW` 获取目录原安全描述符及原 DACL，
/// `SetEntriesInAclW` 将新 ACE 合并到原 DACL，再 `SetNamedSecurityInfoW`
/// 只更新合并后的 DACL。保留原用户/SYSTEM/管理员 ACE 与已有继承策略。
pub fn grant_directory_access(
    profile: &AppContainerProfile,
    readable_dirs: &[PathBuf],
    writable_dirs: &[PathBuf],
) -> Result<(), String> {
    let sid_ptr = profile.sid.as_ptr();

    for dir in readable_dirs.iter().chain(writable_dirs.iter()) {
        if !dir.exists() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("创建目录 {} 失败: {e}", dir.display()))?;
        }

        let dir_wide = wide(&dir.to_string_lossy());
        let access_mode = if writable_dirs.contains(dir) {
            0x1F01FFu32
        } else {
            0x120089u32
        };

        let explicit_access = EXPLICIT_ACCESS_W {
            grfAccessPermissions: access_mode,
            grfAccessMode: SET_ACCESS,
            grfInheritance: 0x03,
            Trustee: TRUSTEE_W {
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_GROUP,
                ptstrName: sid_ptr as *mut _,
                ..Default::default()
            },
        };

        let mut existing_dacl: *mut ACL = ptr::null_mut();
        let mut security_descriptor = PSECURITY_DESCRIPTOR::default();

        unsafe {
            let result = GetNamedSecurityInfoW(
                PCWSTR(dir_wide.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(&mut existing_dacl),
                None,
                &mut security_descriptor,
            );
            if result.0 != 0 {
                return Err(format!(
                    "GetNamedSecurityInfoW for {}: error {}",
                    dir.display(),
                    result.0
                ));
            }

            let mut new_acl: *mut ACL = ptr::null_mut();
            let merge_result = SetEntriesInAclW(
                Some(&[explicit_access]),
                if existing_dacl.is_null() { None } else { Some(existing_dacl) },
                &mut new_acl,
            );
            if merge_result.0 != 0 {
                let _ = windows::Win32::Foundation::LocalFree(HLOCAL(security_descriptor.0));
                return Err(format!("SetEntriesInAclW: error {}", merge_result.0));
            }

            let set_result = SetNamedSecurityInfoW(
                PCWSTR(dir_wide.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(new_acl),
                None,
            );
            let _ = windows::Win32::Foundation::LocalFree(HLOCAL(new_acl.cast()));
            let _ = windows::Win32::Foundation::LocalFree(HLOCAL(security_descriptor.0));

            if set_result.0 != 0 {
                return Err(format!(
                    "SetNamedSecurityInfoW for {}: error {}",
                    dir.display(),
                    set_result.0
                ));
            }
        }
    }

    Ok(())
}

/// 在 AppContainer 中启动受限进程。
///
/// 使用 `CREATE_SUSPENDED` 启动，先 `AssignProcessToJobObject` 再
/// `ResumeThread`，避免进程先运行、后加入 Job 的竞态。
pub fn spawn_in_appcontainer(
    program: &str,
    args: &[String],
    working_dir: &Path,
    capabilities: &SandboxCapabilities<'_>,
    stdin_read: HANDLE,
    stdout_write: HANDLE,
    stderr_write: HANDLE,
) -> Result<SandboxProcess, String> {
    let security_capabilities = capabilities.as_security_capabilities();

    let mut startup_info = STARTUPINFOEXW::default();
    startup_info.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup_info.StartupInfo.dwFlags |= 0x00000100;
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
        .map_err(|e| format!("InitializeProcThreadAttributeList: {e}"))?;

        UpdateProcThreadAttribute(
            startup_info.lpAttributeList,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            Some(&security_capabilities as *const _ as *const _),
            std::mem::size_of::<SECURITY_CAPABILITIES>(),
            None,
            None,
        )
        .map_err(|e| format!("UpdateProcThreadAttribute(CAPABILITIES): {e}"))?;

        let mut inherit_handles = [stdin_read, stdout_write, stderr_write];
        UpdateProcThreadAttribute(
            startup_info.lpAttributeList,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            Some(inherit_handles.as_mut_ptr() as *const _),
            std::mem::size_of::<[HANDLE; 3]>(),
            None,
            None,
        )
        .map_err(|e| format!("UpdateProcThreadAttribute(HANDLE_LIST): {e}"))?;
    }

    let cmd_line = build_command_line(program, args);
    let mut cmd_line_wide = wide(&cmd_line);
    let working_dir_wide = wide(&working_dir.to_string_lossy());

    let job_handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
        .map_err(|e| {
            unsafe { DeleteProcThreadAttributeList(startup_info.lpAttributeList); }
            format!("CreateJobObjectW: {e}")
        })?;

    let mut job_info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    job_info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            job_handle,
            JobObjectExtendedLimitInformation,
            &job_info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .map_err(|e| {
            unsafe {
                DeleteProcThreadAttributeList(startup_info.lpAttributeList);
                let _ = CloseHandle(job_handle);
            }
            format!("SetInformationJobObject: {e}")
        })?;
    }

    let mut process_info = PROCESS_INFORMATION::default();
    let create_result = unsafe {
        CreateProcessW(
            PCWSTR::null(),
            PWSTR(cmd_line_wide.as_mut_ptr()),
            None,
            None,
            true,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED,
            None,
            PCWSTR(working_dir_wide.as_ptr()),
            &mut startup_info.StartupInfo as *mut _ as *mut STARTUPINFOW,
            &mut process_info,
        )
    };

    unsafe { DeleteProcThreadAttributeList(startup_info.lpAttributeList); }

    if let Err(e) = create_result {
        unsafe { let _ = CloseHandle(job_handle); }
        return Err(format!("CreateProcessW: {e}"));
    }

    unsafe {
        let assign_result = AssignProcessToJobObject(job_handle, process_info.hProcess);
        if let Err(e) = assign_result {
            let _ = CloseHandle(process_info.hProcess);
            let _ = CloseHandle(process_info.hThread);
            let _ = CloseHandle(job_handle);
            return Err(format!("AssignProcessToJobObject: {e}"));
        }

        ResumeThread(process_info.hThread);
    }

    Ok(SandboxProcess {
        handle: process_info.hProcess,
        thread_handle: process_info.hThread,
        pid: process_info.dwProcessId,
        job_handle,
    })
}

/// 按 Windows 命令行解析规则编码参数。
///
/// 参考：https://learn.microsoft.com/en-us/archive/blogs/twistylittlepassagesallalike/everyone-quotes-command-line-arguments-the-wrong-way
fn build_command_line(program: &str, args: &[String]) -> String {
    let mut cmd = quote_arg(program);
    for arg in args {
        cmd.push(' ');
        cmd.push_str(&quote_arg(arg));
    }
    cmd
}

fn quote_arg(arg: &str) -> String {
    if !arg.is_empty()
        && !arg.contains(' ')
        && !arg.contains('\t')
        && !arg.contains('"')
        && !arg.contains('\\')
    {
        return arg.to_string();
    }
    let mut result = String::from("\"");
    let mut backslashes = 0;
    for ch in arg.chars() {
        match ch {
            '\\' => {
                backslashes += 1;
            }
            '"' => {
                for _ in 0..2 * backslashes + 1 {
                    result.push('\\');
                }
                result.push('"');
                backslashes = 0;
            }
            _ => {
                for _ in 0..backslashes {
                    result.push('\\');
                }
                result.push(ch);
                backslashes = 0;
            }
        }
    }
    for _ in 0..2 * backslashes {
        result.push('\\');
    }
    result.push('"');
    result
}

#[cfg(test)]
mod tests {
    use super::quote_arg;

    #[test]
    fn quote_arg_no_special() {
        assert_eq!(quote_arg("hello"), "hello");
    }

    #[test]
    fn quote_arg_empty() {
        assert_eq!(quote_arg(""), "\"\"");
    }

    #[test]
    fn quote_arg_space() {
        assert_eq!(quote_arg("hello world"), "\"hello world\"");
    }

    #[test]
    fn quote_arg_backslash() {
        assert_eq!(quote_arg("C:\\path"), "\"C:\\path\"");
    }

    #[test]
    fn quote_arg_trailing_backslash() {
        assert_eq!(quote_arg("C:\\path\\"), "\"C:\\path\\\\\"");
    }

    #[test]
    fn quote_arg_quote_after_backslash() {
        assert_eq!(quote_arg("a\\\"b"), "\"a\\\\\\\"b\"");
    }

    #[test]
    fn quote_arg_multiple_backslashes_before_quote() {
        assert_eq!(quote_arg("a\\\\\"b"), "\"a\\\\\\\\\\\"b\"");
    }

    #[test]
    fn quote_arg_quote_only() {
        assert_eq!(quote_arg("\""), "\"\\\"\"");
    }
}

/// 创建可继承的管道句柄对。
///
/// 子进程端可继承（`bInheritHandle: TRUE`），父进程端不可继承。
pub fn create_inheritable_pipe() -> Result<(HANDLE, HANDLE), String> {
    let mut sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: BOOL(1),
    };
    let mut read = INVALID_HANDLE_VALUE;
    let mut write = INVALID_HANDLE_VALUE;
    unsafe {
        windows::Win32::System::Pipes::CreatePipe(
            &mut read,
            &mut write,
            Some(&sa),
            0,
        )
        .map_err(|e| format!("CreatePipe: {e}"))?;
    }
    Ok((read, write))
}

/// 将句柄标记为不可继承。
pub fn make_non_inheritable(handle: HANDLE) -> Result<(), String> {
    unsafe {
        windows::Win32::Foundation::SetHandleInformation(
            handle,
            windows::Win32::Foundation::HANDLE_FLAG_INHERIT.0,
            windows::Win32::Foundation::HANDLE_FLAGS(0),
        )
        .map_err(|e| format!("SetHandleInformation: {e}"))?;
    }
    Ok(())
}
