using Microsoft.Win32;

namespace DeltaForceFilter.App.Platform;

internal static class StartupRegistration
{
    private const string KeyPath = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string ValueName = "DeltaForceFilterManager";

    internal static bool IsEnabled()
    {
        using var key = Registry.CurrentUser.OpenSubKey(KeyPath);
        return string.Equals(key?.GetValue(ValueName) as string, Command(), StringComparison.Ordinal);
    }

    internal static void SetEnabled(bool enabled)
    {
        using var key = Registry.CurrentUser.CreateSubKey(KeyPath, writable: true);
        if (enabled) key.SetValue(ValueName, Command(), RegistryValueKind.String);
        else key.DeleteValue(ValueName, throwOnMissingValue: false);
        if (IsEnabled() != enabled) throw new IOException("开机启动设置写入后校验失败。");
    }

    private static string Command()
    {
        var executable = Environment.ProcessPath ?? throw new InvalidOperationException("无法获取程序路径。");
        if (!Path.GetFileName(executable).Equals("DeltaForceFilterManager.exe", StringComparison.OrdinalIgnoreCase))
            throw new InvalidOperationException("请从发布目录中的 DeltaForceFilterManager.exe 运行，以配置开机启动。");
        return $"\"{executable}\" --tray";
    }
}
