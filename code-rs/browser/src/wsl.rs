/// WSL2-specific utilities for Chrome DevTools Protocol connections
///
/// WSL2 runs Linux in a VM with separate networking, so Chrome running on the
/// Windows host is not accessible via 127.0.0.1. This module provides utilities
/// to detect WSL2 and discover the correct Windows host IP address.

use std::process::Command;
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
}
