# WSL2 Chrome Connection Setup

This guide explains how to use the `/chrome` and `/browser` commands with Code CLI running in WSL2 (Windows Subsystem for Linux).

## Background

WSL2 runs Linux in a lightweight virtual machine with separate networking from the Windows host. When Chrome runs on Windows and Code CLI runs in WSL2, they cannot communicate via `localhost` (`127.0.0.1`) without additional setup.

As of version 0.4.21, **Code CLI automatically detects WSL2 and uses the correct Windows host IP**, so you don't need to manually specify the IP address anymore!

## Automatic WSL2 Detection (v0.4.21+)

Code CLI now automatically:
- Detects when running in WSL2
- Discovers the Windows host gateway IP via `ip route`
- Uses that IP when connecting to Chrome
- Provides helpful setup instructions if connection fails

## One-Time Windows Setup

You still need to set up port forwarding and firewall rules on Windows (one-time setup):

### Step 1: Find Your WSL Gateway IP

From WSL2, run:
```bash
ip route show | grep default | awk '{print $3}'
```

You'll get an IP like `172.21.48.1`, `172.20.144.1`, or similar. We'll call this `GATEWAY_IP`.

### Step 2: Set Up Port Forwarding

**Run PowerShell as Administrator:**

```powershell
# Replace 172.21.48.1 with your actual GATEWAY_IP from Step 1
netsh interface portproxy add v4tov4 listenport=9222 listenaddress=172.21.48.1 connectport=9222 connectaddress=127.0.0.1

# Verify it's set up
netsh interface portproxy show all
```

This forwards traffic from the WSL network to Chrome running on Windows.

### Step 3: Add Windows Firewall Rule

**Still in PowerShell as Administrator:**

```powershell
# Replace 172.21.48.1 with your actual GATEWAY_IP
New-NetFirewallRule -DisplayName "Chrome Debug Port for WSL2" -Direction Inbound -LocalPort 9222 -Protocol TCP -Action Allow -Profile Any -LocalAddress 172.21.48.1
```

This allows WSL2 to connect through Windows Firewall.

## Usage

### Each Session:

1. **Launch Chrome on Windows with remote debugging:**

   ```powershell
   # In PowerShell on Windows
   & "C:\Program Files\Google\Chrome\Application\chrome.exe" --remote-debugging-port=9222 --user-data-dir=C:\temp\chrome-debug
   ```

   Note: `--user-data-dir` creates a separate Chrome profile so you don't need to close your existing Chrome windows.

2. **Connect from Code CLI in WSL2:**

   ```bash
   # Code will automatically detect WSL2 and use the correct IP
   /chrome 9222

   # Or use auto-detect (scans for Chrome processes)
   /chrome
   ```

   You no longer need to manually specify the IP address!

## Troubleshooting

### Connection Fails

If `/chrome` fails to connect, Code will display a helpful error message with:
- Your detected WSL gateway IP
- Complete Windows setup instructions
- Link to this documentation

### Verify Windows Setup

Check that Chrome is listening and the port proxy is working:

```powershell
# Check Chrome is listening on port 9222
netstat -an | findstr "9222"

# Check port proxy is configured
netsh interface portproxy show all
```

### Test Connection from WSL2

```bash
# Replace GATEWAY_IP with your actual IP
curl http://172.21.48.1:9222/json/version
```

If this returns JSON data, the connection is working!

### WSL Gateway IP Changes

Your WSL gateway IP might change after restarting Windows or WSL2. Code CLI will automatically detect the new IP, but you'll need to update your port forwarding and firewall rules:

```powershell
# Remove old rules
netsh interface portproxy delete v4tov4 listenport=9222 listenaddress=OLD_IP
Remove-NetFirewallRule -DisplayName "Chrome Debug Port for WSL2"

# Add new rules with new IP (see Steps 2 & 3 above)
```

## Technical Details

### How It Works

1. Code CLI detects WSL2 by checking:
   - `WSL_DISTRO_NAME` environment variable
   - `/proc/version` contains "microsoft" or "wsl"

2. When detected, it runs `ip route show` to find the default gateway IP

3. Uses that IP instead of `127.0.0.1` when connecting to Chrome

4. If connection fails, provides setup instructions specific to your WSL configuration

### Code Changes

The WSL2 detection is implemented in `code-rs/browser/src/wsl.rs`:
- `is_wsl()` - Detects WSL2 environment
- `get_wsl_gateway_ip()` - Discovers Windows host IP
- `get_chrome_host()` - Returns appropriate host for connection
- `get_wsl_setup_guide()` - Generates helpful error messages

## Alternative: Use Windows Native Code CLI

If WSL2 networking issues persist, you can also:
1. Install Code CLI natively on Windows (not in WSL2)
2. Use Chrome directly without port forwarding

However, the WSL2 setup provides better integration with Linux-based development workflows.

## Related Issues

- [#278: WSL Support for /chrome or /browser feature](https://github.com/just-every/code/issues/278)
- [#298: How to connect Code to an external Chrome browser](https://github.com/just-every/code/issues/298)

## Cleanup

To remove the port forwarding and firewall rules:

```powershell
# Run as Administrator
netsh interface portproxy delete v4tov4 listenport=9222 listenaddress=YOUR_GATEWAY_IP
Remove-NetFirewallRule -DisplayName "Chrome Debug Port for WSL2"
```
