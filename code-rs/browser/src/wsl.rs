/// WSL2-specific utilities for Chrome DevTools Protocol connections
///
/// WSL2 runs Linux in a VM with separate networking, so Chrome running on the
/// Windows host is not accessible via 127.0.0.1. This module provides utilities
/// to detect WSL2 and discover the correct Windows host IP address.

use std::process::Command;
use std::io::Write;
use tracing::{debug, info, warn};

/// Detect if we're running inside WSL (Windows Subsystem for Linux)
pub fn is_wsl() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Check for WSL_DISTRO_NAME environment variable (WSL2)
        if std::env::var_os("WSL_DISTRO_NAME").is_some() {
            return true;
        }

        // Fallback: check /proc/version for "microsoft" or "WSL"
        match std::fs::read_to_string("/proc/version") {
            Ok(version) => {
                let lower = version.to_lowercase();
                lower.contains("microsoft") || lower.contains("wsl")
            }
            Err(_) => false,
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Get the Windows host gateway IP address from WSL2
///
/// In WSL2, the Windows host is accessible via the default gateway IP.
/// This function uses `ip route` to discover that IP address.
///
/// Returns None if:
/// - Not running in WSL
/// - Cannot execute `ip route` command
/// - Cannot parse the gateway IP
pub fn get_wsl_gateway_ip() -> Option<String> {
    if !is_wsl() {
        return None;
    }

    debug!("[wsl] Detecting WSL2 gateway IP via 'ip route'");

    // Run: ip route show | grep default
    let output = Command::new("ip")
        .args(&["route", "show"])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Find the default route line and extract the gateway IP
    for line in stdout.lines() {
        if line.contains("default") {
            // Format: "default via 172.21.48.1 dev eth0"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(via_idx) = parts.iter().position(|&p| p == "via") {
                if let Some(&gateway) = parts.get(via_idx + 1) {
                    info!("[wsl] Detected Windows host gateway IP: {}", gateway);
                    return Some(gateway.to_string());
                }
            }
        }
    }

    warn!("[wsl] Could not detect WSL2 gateway IP from 'ip route show'");
    None
}

/// Get the appropriate host address for Chrome CDP connections
///
/// Returns:
/// - WSL2 gateway IP if running in WSL2 and no explicit host provided
/// - The provided host if specified
/// - "127.0.0.1" as fallback
pub fn get_chrome_host(explicit_host: Option<&str>) -> String {
    // If user explicitly specified a host, use it
    if let Some(host) = explicit_host {
        return host.to_string();
    }

    // In WSL2, try to auto-detect the Windows host gateway
    if let Some(gateway) = get_wsl_gateway_ip() {
        info!("[wsl] Using WSL2 gateway IP for Chrome connection: {}", gateway);
        return gateway;
    }

    // Default to localhost
    "127.0.0.1".to_string()
}

/// Query Windows Registry for Chrome installation path
///
/// Uses PowerShell to query the registry for the Chrome installation path
fn query_chrome_registry_path() -> Option<String> {
    // Try to get Chrome path from Windows Registry via PowerShell
    let registry_queries = vec![
        // Google Chrome
        r"(Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\chrome.exe' -ErrorAction SilentlyContinue).'(Default)'",
        r"(Get-ItemProperty -Path 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths\chrome.exe' -ErrorAction SilentlyContinue).'(Default)'",
        // Microsoft Edge
        r"(Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe' -ErrorAction SilentlyContinue).'(Default)'",
    ];

    for query in registry_queries {
        if let Ok(output) = Command::new("powershell.exe")
            .args(&["-NoProfile", "-Command", query])
            .output()
        {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() && path != "null" && !path.contains("error") {
                debug!("[wsl] Found Chrome via registry: {}", path);
                // Verify the path exists
                if verify_chrome_path(&path) {
                    return Some(path);
                }
            }
        }
    }

    None
}

/// Verify that a Chrome executable path is valid
///
/// Checks if the file exists and is executable
fn verify_chrome_path(windows_path: &str) -> bool {
    // Convert Windows path to WSL path for checking
    // Handle any drive letter (C:, D:, E:, etc.)
    let wsl_path = windows_to_wsl_path(windows_path);

    let path = std::path::Path::new(&wsl_path);

    if !path.exists() {
        debug!("[wsl] Path does not exist: {}", wsl_path);
        return false;
    }

    if !path.is_file() {
        debug!("[wsl] Path is not a file: {}", wsl_path);
        return false;
    }

    // Check if it's executable (on Windows, .exe files are executable)
    if !wsl_path.to_lowercase().ends_with(".exe") {
        debug!("[wsl] Path is not an executable: {}", wsl_path);
        return false;
    }

    debug!("[wsl] Verified Chrome path: {}", windows_path);
    true
}

/// Convert Windows path to WSL path
///
/// Handles any drive letter (C:, D:, E:, etc.)
fn windows_to_wsl_path(windows_path: &str) -> String {
    let path = windows_path.trim();

    // Handle paths like "C:\Program Files\..." or "C:/Program Files/..."
    if path.len() >= 3 && path.chars().nth(1) == Some(':') {
        let drive = path.chars().nth(0).unwrap().to_lowercase();
        let rest = &path[2..].replace('\\', "/");
        format!("/mnt/{}{}", drive, rest)
    } else {
        // Already a WSL-style path or invalid
        path.replace('\\', "/")
    }
}

/// Detect Chrome installation path on Windows from WSL2
///
/// Tries multiple methods in order of reliability:
/// 1. Windows Registry query (most reliable)
/// 2. Common installation paths across multiple drives
/// 3. Returns None if not found (user can specify manually)
pub fn detect_chrome_path() -> Option<String> {
    if !is_wsl() {
        return None;
    }

    info!("[wsl] Detecting Chrome installation path...");

    // Method 1: Try Windows Registry (most reliable)
    if let Some(path) = query_chrome_registry_path() {
        info!("[wsl] Chrome detected via registry: {}", path);
        return Some(path);
    }

    // Method 2: Check common paths across multiple drives
    let drives = vec!["C", "D", "E", "F"];
    let browser_paths = vec![
        // Google Chrome
        (r"\Program Files\Google\Chrome\Application\chrome.exe", "Google Chrome"),
        (r"\Program Files (x86)\Google\Chrome\Application\chrome.exe", "Google Chrome (x86)"),
        // Microsoft Edge
        (r"\Program Files (x86)\Microsoft\Edge\Application\msedge.exe", "Microsoft Edge (x86)"),
        (r"\Program Files\Microsoft\Edge\Application\msedge.exe", "Microsoft Edge"),
        // Chromium
        (r"\Program Files\Chromium\Application\chrome.exe", "Chromium"),
        (r"\Program Files (x86)\Chromium\Application\chrome.exe", "Chromium (x86)"),
        // Brave
        (r"\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe", "Brave"),
        (r"\Program Files (x86)\BraveSoftware\Brave-Browser\Application\brave.exe", "Brave (x86)"),
    ];

    for drive in drives {
        for (path_suffix, name) in &browser_paths {
            let windows_path = format!("{}:{}", drive, path_suffix);

            if verify_chrome_path(&windows_path) {
                info!("[wsl] Chrome detected at {}: {}", drive, name);
                return Some(windows_path);
            }
        }
    }

    warn!("[wsl] Could not auto-detect Chrome installation path");
    warn!("[wsl] User will need to specify Chrome path manually");
    None
}

/// Get the Windows username from WSL2
///
/// This is useful for generating paths on the Windows filesystem
pub fn get_windows_username() -> Option<String> {
    if !is_wsl() {
        return None;
    }

    // Try to get from WSLENV or infer from /mnt/c/Users
    if let Ok(output) = Command::new("cmd.exe")
        .args(&["/c", "echo", "%USERNAME%"])
        .output()
    {
        let username = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();
        if !username.is_empty() && username != "%USERNAME%" {
            return Some(username);
        }
    }

    // Fallback: try to find from home directory
    if let Ok(output) = Command::new("cmd.exe")
        .args(&["/c", "echo", "%USERPROFILE%"])
        .output()
    {
        let profile = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if let Some(username) = profile.split('\\').last() {
            return Some(username.to_string());
        }
    }

    None
}

/// Test if Chrome is reachable on the given host and port
///
/// Returns true if we can successfully connect to the Chrome debug port
pub async fn test_chrome_connection(host: &str, port: u16) -> bool {
    use reqwest::Client;
    use std::time::Duration;

    let url = format!("http://{}:{}/json/version", host, port);
    debug!("[wsl] Testing Chrome connection to {}", url);

    let client = match Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            info!("[wsl] Chrome connection test successful");
            true
        }
        Ok(resp) => {
            debug!("[wsl] Chrome connection test failed with status: {}", resp.status());
            false
        }
        Err(e) => {
            debug!("[wsl] Chrome connection test failed: {}", e);
            false
        }
    }
}

/// Generate PowerShell setup script content for WSL2 Chrome connection
///
/// Returns the complete PowerShell script as a string
pub fn generate_setup_script(gateway_ip: &str, port: u16) -> String {
    let chrome_path = detect_chrome_path()
        .unwrap_or_else(|| r"C:\Program Files\Google\Chrome\Application\chrome.exe".to_string());

    format!(
        r#"# WSL2 Chrome Connection Setup Script
# Generated by Code CLI
# This script sets up port forwarding and firewall rules for Chrome DevTools Protocol

Write-Host "Setting up WSL2 Chrome connection..." -ForegroundColor Cyan
Write-Host ""

# Check if running as Administrator
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {{
    Write-Host "ERROR: This script must be run as Administrator!" -ForegroundColor Red
    Write-Host "Right-click PowerShell and select 'Run as Administrator'" -ForegroundColor Yellow
    Read-Host "Press Enter to exit"
    exit 1
}}

Write-Host "Step 1: Setting up port forwarding..." -ForegroundColor Green
try {{
    # Remove any existing port proxy on this port
    netsh interface portproxy delete v4tov4 listenport={port} listenaddress={gateway_ip} 2>$null

    # Add new port proxy
    netsh interface portproxy add v4tov4 listenport={port} listenaddress={gateway_ip} connectport={port} connectaddress=127.0.0.1

    Write-Host "  Port forwarding configured: {gateway_ip}:{port} -> 127.0.0.1:{port}" -ForegroundColor White
}} catch {{
    Write-Host "  ERROR: Failed to set up port forwarding: $_" -ForegroundColor Red
    Read-Host "Press Enter to exit"
    exit 1
}}

Write-Host ""
Write-Host "Step 2: Configuring Windows Firewall..." -ForegroundColor Green
try {{
    # Remove any existing rule with this name
    Remove-NetFirewallRule -DisplayName "Chrome Debug Port for WSL2" -ErrorAction SilentlyContinue

    # Add new firewall rule
    New-NetFirewallRule -DisplayName "Chrome Debug Port for WSL2" `
        -Direction Inbound `
        -LocalPort {port} `
        -Protocol TCP `
        -Action Allow `
        -Profile Any `
        -LocalAddress {gateway_ip} | Out-Null

    Write-Host "  Firewall rule added for port {port}" -ForegroundColor White
}} catch {{
    Write-Host "  ERROR: Failed to configure firewall: $_" -ForegroundColor Red
    Read-Host "Press Enter to exit"
    exit 1
}}

Write-Host ""
Write-Host "Setup complete!" -ForegroundColor Green
Write-Host ""
Write-Host "Next steps:" -ForegroundColor Cyan
Write-Host "1. Launch Chrome with remote debugging:" -ForegroundColor White
Write-Host '   & "{chrome_path}" --remote-debugging-port={port} --user-data-dir=C:\temp\chrome-debug' -ForegroundColor Yellow
Write-Host ""
Write-Host "2. From WSL2, run:" -ForegroundColor White
Write-Host "   /chrome {port}" -ForegroundColor Yellow
Write-Host ""

Read-Host "Press Enter to close"
"#,
        gateway_ip = gateway_ip,
        port = port,
        chrome_path = chrome_path
    )
}

/// Save the PowerShell setup script to the Windows filesystem
///
/// Returns the Windows path where the script was saved
pub fn save_setup_script(gateway_ip: &str, port: u16) -> Result<String, std::io::Error> {
    let script_content = generate_setup_script(gateway_ip, port);

    // Try to get Windows username
    let username = get_windows_username().unwrap_or_else(|| "UNKNOWN".to_string());

    // Generate Windows path (accessible from WSL)
    let wsl_path = format!("/mnt/c/Users/{}/chrome-wsl2-setup.ps1", username);
    let windows_path = format!("C:\\Users\\{}\\chrome-wsl2-setup.ps1", username);

    // Write the script
    let mut file = std::fs::File::create(&wsl_path)?;
    file.write_all(script_content.as_bytes())?;

    info!("[wsl] Saved setup script to {}", windows_path);
    Ok(windows_path)
}

/// Generate a helpful error message for WSL2 users when Chrome connection fails
pub fn get_wsl_setup_guide() -> Option<String> {
    if !is_wsl() {
        return None;
    }

    let gateway = get_wsl_gateway_ip().unwrap_or_else(|| "GATEWAY_IP".to_string());

    Some(format!(
        r#"
WSL2 Chrome Connection Setup Required
======================================

Chrome is running on Windows, but Code is running in WSL2. To connect them:

1. Launch Chrome on Windows with remote debugging:

   PowerShell:
   & "C:\Program Files\Google\Chrome\Application\chrome.exe" --remote-debugging-port=9222

2. Set up port forwarding (ONE-TIME, run as Administrator):

   PowerShell:
   netsh interface portproxy add v4tov4 listenport=9222 listenaddress={gateway} connectport=9222 connectaddress=127.0.0.1

3. Add Windows Firewall rule (ONE-TIME, run as Administrator):

   PowerShell:
   New-NetFirewallRule -DisplayName "Chrome Debug Port for WSL2" -Direction Inbound -LocalPort 9222 -Protocol TCP -Action Allow -Profile Any -LocalAddress {gateway}

4. Then connect from Code:
   /chrome {gateway}:9222

For more details, see: https://github.com/just-every/code/issues/278
"#,
        gateway = gateway
    ))
}

/// Get interactive setup instructions for WSL2 users
pub fn get_wsl_interactive_guide(gateway_ip: &str, port: u16, script_path: &str) -> String {
    let chrome_path = detect_chrome_path();

    let (chrome_cmd, chrome_note) = if let Some(path) = &chrome_path {
        (
            format!(r#"& "{}" --remote-debugging-port={} --user-data-dir=C:\temp\chrome-debug"#, path, port),
            format!("✓ Chrome detected at: {}", path)
        )
    } else {
        (
            format!(r#"& "C:\Path\To\Your\Chrome.exe" --remote-debugging-port={} --user-data-dir=C:\temp\chrome-debug"#, port),
            "⚠ Chrome path not auto-detected. Please update the path above.

To find your Chrome installation:
  - Open PowerShell and run: Get-Command chrome.exe | Select-Object Source
  - Or check: C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe
  - Or use Edge: C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe".to_string()
        )
    };

    format!(
        r#"
🔍 WSL2 Detected - Chrome Connection Setup Needed
================================================

To connect to Chrome on Windows from WSL2, you need one-time setup:

Option 1: Run the generated PowerShell script (EASIEST)
--------------------------------------------------------
A setup script has been created at:
  {}

To run it:
  1. Open PowerShell as Administrator on Windows
  2. Run: {}
  3. Follow the prompts

The script will automatically configure port forwarding and firewall rules.

Option 2: Manual setup (copy/paste these commands)
---------------------------------------------------
In PowerShell as Administrator, run:

# Port forwarding
netsh interface portproxy add v4tov4 listenport={} listenaddress={} connectport={} connectaddress=127.0.0.1

# Firewall rule
New-NetFirewallRule -DisplayName "Chrome Debug Port for WSL2" -Direction Inbound -LocalPort {} -Protocol TCP -Action Allow -Profile Any -LocalAddress {}

After setup, launch Chrome on Windows:
---------------------------------------
{}

{}

Then run '/chrome {}' again from Code CLI.

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
This is a ONE-TIME setup. Port forwarding and firewall rules persist across reboots.
After setup, you only need to launch Chrome each session.
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
"#,
        script_path,
        script_path,
        port,
        gateway_ip,
        port,
        port,
        gateway_ip,
        chrome_cmd,
        chrome_note,
        port
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_chrome_host_explicit() {
        assert_eq!(get_chrome_host(Some("192.168.1.1")), "192.168.1.1");
        assert_eq!(get_chrome_host(Some("example.com")), "example.com");
    }

    #[test]
    fn test_get_chrome_host_fallback() {
        // When not in WSL and no explicit host, should default to 127.0.0.1
        #[cfg(not(target_os = "linux"))]
        {
            assert_eq!(get_chrome_host(None), "127.0.0.1");
        }
    }

    #[test]
    fn test_windows_to_wsl_path() {
        assert_eq!(
            windows_to_wsl_path(r"C:\Program Files\Chrome\chrome.exe"),
            "/mnt/c/Program Files/Chrome/chrome.exe"
        );
        assert_eq!(
            windows_to_wsl_path(r"D:\Apps\Chrome\chrome.exe"),
            "/mnt/d/Apps/Chrome/chrome.exe"
        );
        assert_eq!(
            windows_to_wsl_path("C:/Program Files/Chrome/chrome.exe"),
            "/mnt/c/Program Files/Chrome/chrome.exe"
        );
        // Test already WSL path
        assert_eq!(
            windows_to_wsl_path("/mnt/c/test"),
            "/mnt/c/test"
        );
    }

    #[test]
    fn test_generate_setup_script() {
        let script = generate_setup_script("172.21.48.1", 9222);

        // Check that script contains key elements
        assert!(script.contains("172.21.48.1"));
        assert!(script.contains("9222"));
        assert!(script.contains("netsh interface portproxy"));
        assert!(script.contains("New-NetFirewallRule"));
        assert!(script.contains("Administrator"));
    }
}
