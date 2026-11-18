//! Windows AppContainer 沙箱外的可信启动器。
//!
//! 接收 `--agent-id`、`--working-dir`、`--launch-path`、`--readable-dir`、
//! `--writable-dir` 和 `--` 后透传的 CLI 参数。在 AppContainer 中启动 CLI，
//! 转发 stdin/stdout/stderr，管理进程生命周期。
//!
//! 从宿主进程角度看，`sujian-agent-host.exe` 与直接启动 CLI 行为一致：
//! stdin/stdout/stderr 是标准管道，退出码反映 CLI 真实退出状态。

use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr;
use std::thread;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::CreatePipe;
use windows::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, UpdateProcThreadAttribute, WaitForSingleObject,
    EXTENDED_STARTUPINFO_PRESENT, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, PROCESS_INFORMATION, STARTUPINFOEXW,
    STARTUPINFOW, WAIT_OBJECT_0,
};
use windows::Win32::Security::{
    DeriveAppContainerSidFromAppContainerName, SECURITY_CAPAB1_0, SID_AND_ATTRIBUTES,
    WELL_KNOWN_SID_TYPE,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

const APPCONTAINER_PREFIX: &str = "Sujian.Agent.";

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

struct AppContainerProfile {
    name: String,
    sid: Vec<u8>,
}

impl AppContainerProfile {
    fn for_agent(agent_id: &str) -> Result<Self, String> {
        let name = format!("{APPCONTAINER_PREFIX}{agent_id}");
        let name_wide = wide(&name);

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
                .map_err(|e| format!("DeriveAppContainerSidFromAppContainerName: {e}"))?;
            }
        }

        sid_buffer.truncate(sid_len as usize);
        Ok(Self {
            name,
            sid: sid_buffer,
        })
    }
}

fn create_well_known_sid(sid_type: WELL_KNOWN_SID_TYPE) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; 256];
    let mut len = buf.len() as u32;
    unsafe {
        windows::Win32::Security::CreateWellKnownSid(
            sid_type,
            ptr::null(),
            buf.as_mut_ptr() as *mut _,
            &mut len,
        )
        .map_err(|e| format!("CreateWellKnownSid: {e}"))?;
    }
    buf.truncate(len as usize);
    Ok(buf)
}

fn build_security_capabilities(profile: &AppContainerProfile) -> Result<SECURITY_CAPAB1_0, String> {
    let internet_sid =
        create_well_known_sid(WELL_KNOWN_SID_TYPE::WinCapabilityInternetClientSid)?;
    let private_network_sid =
        create_well_known_sid(WELL_KNOWN_SID_TYPE::WinCapabilityPrivateNetworkClientServerSid)?;

    let mut capabilities = vec![
        SID_AND_ATTRIBUTES {
            Sid: internet_sid.as_ptr() as *mut _,
            Attributes: 0,
        },
        SID_AND_ATTRIBUTES {
            Sid: private_network_sid.as_ptr() as *mut _,
            Attributes: 0,
        },
    ];

    Ok(SECURITY_CAPAB1_0 {
        AppContainerSid: profile.sid.as_ptr() as *mut _,
        Capabilities: capabilities.as_mut_ptr(),
        CapabilityCount: capabilities.len() as u32,
        Reserved: 0,
    })
}

fn grant_directory_access(
    profile: &AppContainerProfile,
    readable_dirs: &[PathBuf],
    writable_dirs: &[PathBuf],
) -> Result<(), String> {
    use windows::Win32::Security::Authorization::{
        SetEntriesInAclW, EXPLICIT_ACCESS_W, SET_ACCESS, TRUSTEE_IS_GROUP, TRUSTEE_IS_SID,
        TRUSTEE_W,
    };
    use windows::Win32::Security::{
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    };
    use windows::Win32::Storage::FileSystem::SetNamedSecurityInfoW;

    let sid_ptr = profile.sid.as_ptr() as *const _;

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

        let mut new_acl: windows::Win32::Security::PACL = ptr::null_mut();
        unsafe {
            let r = SetEntriesInAclW(1, &explicit_access, ptr::null(), &mut new_acl);
            if r != 0 {
                return Err(format!("SetEntriesInAclW: error {r}"));
            }
            let r = SetNamedSecurityInfoW(
                PCWSTR(dir_wide.as_ptr()),
                windows::Win32::Storage::FileSystem::SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                Some(new_acl),
                ptr::null_mut(),
            );
            let _ = windows::Win32::Foundation::LocalFree(new_acl as *mut _);
            if r.is_err() {
                return Err(format!(
                    "SetNamedSecurityInfoW for {}: {:?}",
                    dir.display(),
                    r.unwrap_err()
                ));
            }
        }
    }
    Ok(())
}

fn spawn_in_appcontainer(
    program: &str,
    args: &[String],
    working_dir: &PathBuf,
    profile: &AppContainerProfile,
    stdin_read: HANDLE,
    stdout_write: HANDLE,
    stderr_write: HANDLE,
) -> Result<(HANDLE, HANDLE, u32, HANDLE), String> {
    let capabilities = build_security_capabilities(profile)?;

    let mut startup_info = STARTUPINFOEXW::default();
    startup_info.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup_info.StartupInfo.dwFlags |= 0x00000100; // STARTF_USESTDHANDLES
    startup_info.StartupInfo.hStdInput = stdin_read;
    startup_info.StartupInfo.hStdOutput = stdout_write;
    startup_info.StartupInfo.hStdError = stderr_write;

    let attr_count = 2u32;
    let mut attr_size: usize = 0;
    unsafe {
        InitializeProcThreadAttributeList(ptr::null_mut(), attr_count, 0, &mut attr_size)
    };
    let mut attr_buf = vec![0u8; attr_size];
    startup_info.lpAttributeList = attr_buf.as_mut_ptr() as *mut _;

    unsafe {
        InitializeProcThreadAttributeList(
            startup_info.lpAttributeList,
            attr_count,
            0,
            &mut attr_size,
        )
        .map_err(|e| format!("InitializeProcThreadAttributeList: {e}"))?;

        UpdateProcThreadAttribute(
            startup_info.lpAttributeList,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            &capabilities as *const _ as *const _,
            std::mem::size_of::<SECURITY_CAPAB1_0>(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
        .map_err(|e| format!("UpdateProcThreadAttribute(CAPABILITIES): {e}"))?;

        let mut inherit = [stdin_read, stdout_write, stderr_write];
        UpdateProcThreadAttribute(
            startup_info.lpAttributeList,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            inherit.as_mut_ptr() as *const _,
            std::mem::size_of::<[HANDLE; 3]>(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
        .map_err(|e| format!("UpdateProcThreadAttribute(HANDLE_LIST): {e}"))?;
    }

    let mut cmd = format!("\"{program}\"");
    for arg in args {
        cmd.push(' ');
        if arg.contains(' ') {
            cmd.push('"');
            cmd.push_str(arg);
            cmd.push('"');
        } else {
            cmd.push_str(arg);
        }
    }
    let mut cmd_wide = wide(&cmd);
    let dir_wide = wide(&working_dir.to_string_lossy());

    let job = unsafe { CreateJobObjectW(ptr::null(), PCWSTR(ptr::null())) };
    if job == INVALID_HANDLE_VALUE {
        unsafe { DeleteProcThreadAttributeList(startup_info.lpAttributeList) };
        return Err("CreateJobObjectW failed".to_string());
    }
    let mut job_info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    job_info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &job_info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };

    let mut proc_info = PROCESS_INFORMATION::default();
    let result = unsafe {
        CreateProcessW(
            PCWSTR(ptr::null()),
            cmd_wide.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            true,
            EXTENDED_STARTUPINFO_PRESENT,
            ptr::null(),
            PCWSTR(dir_wide.as_ptr()),
            &mut startup_info.StartupInfo as *mut _ as *mut STARTUPINFOW,
            &mut proc_info,
        )
    };

    unsafe { DeleteProcThreadAttributeList(startup_info.lpAttributeList) };

    if result.is_err() {
        unsafe { let _ = CloseHandle(job) };
        return Err(format!("CreateProcessW: {:?}", result.unwrap_err()));
    }

    unsafe {
        let _ = AssignProcessToJobObject(job, proc_info.hProcess);
    }

    Ok((proc_info.hProcess, proc_info.hThread, proc_info.dwProcessId, job))
}

fn parse_args() -> Result<(String, PathBuf, String, Vec<PathBuf>, Vec<PathBuf>, Vec<String>), String> {
    let mut agent_id = String::new();
    let mut working_dir = PathBuf::new();
    let mut launch_path = String::new();
    let mut readable_dirs = Vec::new();
    let mut writable_dirs = Vec::new();
    let mut cli_args = Vec::new();
    let mut after_separator = false;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        if after_separator {
            cli_args.push(args[i].clone());
            i += 1;
            continue;
        }
        match args[i].as_str() {
            "--agent-id" => {
                i += 1;
                if i < args.len() {
                    agent_id = args[i].clone();
                }
            }
            "--working-dir" => {
                i += 1;
                if i < args.len() {
                    working_dir = PathBuf::from(&args[i]);
                }
            }
            "--launch-path" => {
                i += 1;
                if i < args.len() {
                    launch_path = args[i].clone();
                }
            }
            "--readable-dir" => {
                i += 1;
                if i < args.len() {
                    readable_dirs.push(PathBuf::from(&args[i]));
                }
            }
            "--writable-dir" => {
                i += 1;
                if i < args.len() {
                    writable_dirs.push(PathBuf::from(&args[i]));
                }
            }
            "--" => {
                after_separator = true;
            }
            _ => {
                return Err(format!("未知参数: {}", args[i]));
            }
        }
        i += 1;
    }

    if agent_id.is_empty() {
        return Err("--agent-id 是必需参数".to_string());
    }
    if working_dir.as_os_str().is_empty() {
        return Err("--working-dir 是必需参数".to_string());
    }
    if launch_path.is_empty() {
        return Err("--launch-path 是必需参数".to_string());
    }

    Ok((agent_id, working_dir, launch_path, readable_dirs, writable_dirs, cli_args))
}

fn main() {
    let (agent_id, working_dir, launch_path, readable_dirs, writable_dirs, cli_args) =
        match parse_args() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("sujian-agent-host: {e}");
                std::process::exit(2);
            }
        };

    let profile = match AppContainerProfile::for_agent(&agent_id) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("sujian-agent-host: 创建 AppContainer profile 失败: {e}");
            std::process::exit(3);
        }
    };

    if let Err(e) = grant_directory_access(&profile, &readable_dirs, &writable_dirs) {
        eprintln!("sujian-agent-host: 设置目录 ACL 失败: {e}");
        std::process::exit(4);
    }

    let mut stdin_read = INVALID_HANDLE_VALUE;
    let mut stdin_write = INVALID_HANDLE_VALUE;
    let mut stdout_read = INVALID_HANDLE_VALUE;
    let mut stdout_write = INVALID_HANDLE_VALUE;
    let mut stderr_read = INVALID_HANDLE_VALUE;
    let mut stderr_write = INVALID_HANDLE_VALUE;

    unsafe {
        CreatePipe(&mut stdin_read, &mut stdin_write, ptr::null(), 0);
        CreatePipe(&mut stdout_read, &mut stdout_write, ptr::null(), 0);
        CreatePipe(&mut stderr_read, &mut stderr_write, ptr::null(), 0);
    }

    let (proc_handle, thread_handle, _pid, job_handle) = match spawn_in_appcontainer(
        &launch_path,
        &cli_args,
        &working_dir,
        &profile,
        stdin_read,
        stdout_write,
        stderr_write,
    ) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("sujian-agent-host: 启动沙箱进程失败: {e}");
            unsafe {
                let _ = CloseHandle(stdin_read);
                let _ = CloseHandle(stdin_write);
                let _ = CloseHandle(stdout_read);
                let _ = CloseHandle(stdout_write);
                let _ = CloseHandle(stderr_read);
                let _ = CloseHandle(stderr_write);
            }
            std::process::exit(5);
        }
    };

    unsafe {
        let _ = CloseHandle(stdin_read);
        let _ = CloseHandle(stdout_write);
        let _ = CloseHandle(stderr_write);
    }

    let stdin_handle = stdin_write;
    let stdout_handle = stdout_read;
    let stderr_handle = stderr_read;

    let stdin_thread = thread::spawn(move || {
        let mut stdin = io::stdin();
        let mut buf = [0u8; 4096];
        loop {
            match stdin.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    unsafe {
                        let mut written: u32 = 0;
                        windows::Win32::Storage::FileSystem::WriteFile(
                            stdin_handle,
                            Some(&buf[..n]),
                            &mut written,
                            ptr::null(),
                        )
                        .ok();
                    }
                }
                Err(_) => break,
            }
        }
        unsafe { let _ = CloseHandle(stdin_handle); }
    });

    let stdout_thread = thread::spawn(move || {
        let mut stdout = io::stdout();
        let mut buf = [0u8; 4096];
        loop {
            let mut read: u32 = 0;
            let result = unsafe {
                windows::Win32::Storage::FileSystem::ReadFile(
                    stdout_handle,
                    Some(&mut buf),
                    &mut read,
                    ptr::null(),
                )
            };
            if result.is_err() || read == 0 {
                break;
            }
            if stdout.write_all(&buf[..read as usize]).is_err() {
                break;
            }
            let _ = stdout.flush();
        }
        unsafe { let _ = CloseHandle(stdout_handle); }
    });

    let stderr_thread = thread::spawn(move || {
        let mut stderr = io::stderr();
        let mut buf = [0u8; 4096];
        loop {
            let mut read: u32 = 0;
            let result = unsafe {
                windows::Win32::Storage::FileSystem::ReadFile(
                    stderr_handle,
                    Some(&mut buf),
                    &mut read,
                    ptr::null(),
                )
            };
            if result.is_err() || read == 0 {
                break;
            }
            let _ = stderr.write_all(&buf[..read as usize]);
        }
        unsafe { let _ = CloseHandle(stderr_handle); }
    });

    unsafe {
        let result = WaitForSingleObject(proc_handle, u32::MAX);
        let exit_code = if result == WAIT_OBJECT_0 {
            let mut code: u32 = 0;
            GetExitCodeProcess(proc_handle, &mut code).ok();
            code
        } else {
            1
        };

        let _ = CloseHandle(proc_handle);
        let _ = CloseHandle(thread_handle);
        let _ = CloseHandle(job_handle);

        stdin_thread.join().ok();
        stdout_thread.join().ok();
        stderr_thread.join().ok();

        std::process::exit(exit_code as i32);
    }
}
