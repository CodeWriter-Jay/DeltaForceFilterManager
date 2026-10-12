using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
using DeltaForceFilter.Core;

namespace DeltaForceFilter.App.Platform;

internal sealed class WindowsDisplayBackend(string profileDirectory) : IDisplayBackend
{
    private const uint MonitorClass = 0x6D6E7472;
    private const uint CurrentUser = 1;
    private const uint IccType = 0;
    private const uint ProfileSubtype = 4; // CPST_NONE for a device's default ICC.

    internal static List<MonitorTarget> Monitors()
    {
        var results = new List<MonitorTarget>();
        for (uint index = 0; ; index++)
        {
            var adapter = NativeMethods.DisplayDevice.Create();
            if (!NativeMethods.EnumDisplayDevices(null, index, ref adapter, 0)) break;
            if ((adapter.Flags & 1) == 0 || (adapter.Flags & 8) != 0) continue;
            // Gamma belongs to a GDI source. Clone displays share the same ramp.
            var monitor = NativeMethods.DisplayDevice.Create();
            if (!NativeMethods.EnumDisplayDevices(adapter.Name, 0, ref monitor, 0) || string.IsNullOrWhiteSpace(monitor.Id)) continue;
            results.Add(new(adapter.Name, monitor.Id, $"{monitor.Description}  ·  {adapter.Name.Replace(@"\\.\", "")}{((adapter.Flags & 4) != 0 ? " · 主屏" : "")}", (adapter.Flags & 4) != 0));
        }
        return results.OrderByDescending(m => m.Primary).ToList();
    }

    private static MonitorTarget Resolve(MonitorTarget monitor) => Monitors().FirstOrDefault(m => m.MonitorId == monitor.MonitorId)
        ?? throw new InvalidOperationException("目标显示器已断开，恢复备份会保留。请重新连接原显示器后恢复。");

    internal static bool AdvancedColorEnabled(MonitorTarget monitor)
    {
        const uint onlyActive = 2;
        for (var attempt = 0; attempt < 3; attempt++)
        {
            CheckCode(NativeMethods.GetDisplayConfigBufferSizes(onlyActive, out var pathsCount, out var modesCount), "读取显示拓扑");
            if (pathsCount > 256 || modesCount > 1024) throw new InvalidOperationException("显示拓扑异常。");
            var paths = new NativeMethods.DisplayPath[pathsCount];
            var modes = new NativeMethods.DisplayMode[modesCount];
            var code = NativeMethods.QueryDisplayConfig(onlyActive, ref pathsCount, paths, ref modesCount, modes, 0);
            if (code == 122) continue; // display hotplug raced buffer allocation
            CheckCode(code, "读取显示拓扑");
            var found = false;
            foreach (var path in paths.Take((int)pathsCount))
            {
                var source = new NativeMethods.SourceName
                {
                    Header = new() { Type = 1, Size = (uint)Marshal.SizeOf<NativeMethods.SourceName>(), Adapter = path.Source.Adapter, Id = path.Source.Id },
                    Name = ""
                };
                CheckCode(NativeMethods.GetSourceName(ref source), "读取显示器名称");
                if (!string.Equals(source.Name, monitor.DeviceName, StringComparison.OrdinalIgnoreCase)) continue;
                found = true;
                var color = new NativeMethods.AdvancedColor
                {
                    Header = new() { Type = 9, Size = (uint)Marshal.SizeOf<NativeMethods.AdvancedColor>(), Adapter = path.Target.Adapter, Id = path.Target.Id }
                };
                CheckCode(NativeMethods.GetAdvancedColor(ref color), "检查 HDR/高级色彩");
                if ((color.Value & 2) != 0) return true;
            }
            if (found) return false;
            throw new InvalidOperationException("无法匹配所选显示器的显示拓扑。");
        }
        throw new InvalidOperationException("显示拓扑正在变化，请稍后重试。");
    }

    internal static string NvidiaStatus(MonitorTarget monitor)
    {
        try
        {
            using var nvidia = new NvidiaController();
            return $"NVIDIA 已连接 · 当前数字振动 {nvidia.ReadPercent(monitor.DeviceName)}%";
        }
        catch (Exception error) when (error is DllNotFoundException or EntryPointNotFoundException or NotSupportedException)
        {
            return $"数字振动不可用：{error.Message}";
        }
    }

    public DisplaySnapshot Capture(MonitorTarget monitor, bool requireVibrance)
    {
        monitor = Resolve(monitor);
        if (AdvancedColorEnabled(monitor)) throw new NotSupportedException("请先关闭所选显示器的 HDR/高级色彩，再使用 SDR 滤镜。");
        Check(NativeMethods.WcsGetUsePerUserProfiles(monitor.MonitorId, MonitorClass, out var perUser), "读取 ICC 用户配置状态");
        int? vibrance = null;
        try
        {
            using var nvidia = new NvidiaController();
            vibrance = nvidia.ReadRaw(monitor.DeviceName);
        }
        catch (Exception error) when (!requireVibrance && error is DllNotFoundException or EntryPointNotFoundException or NotSupportedException)
        {
            // Only optional capability detection may continue without a NVIDIA backup.
        }
        return new()
        {
            Monitor = monitor,
            GammaRamp = ReadRamp(monitor),
            PerUserProfiles = perUser,
            UserIccProfile = DefaultProfile(monitor.MonitorId),
            NvidiaVibranceRaw = vibrance
        };
    }

    public void Apply(MonitorTarget monitor, FilterProfile profile, ushort[] ramp)
    {
        monitor = Resolve(monitor);
        if (AdvancedColorEnabled(monitor)) throw new NotSupportedException("检测到 HDR/高级色彩，已停止应用 SDR 曲线。");
        if (profile.IccPath is not null)
        {
            var installed = EnsureInstalled(profile.IccPath);
            Check(NativeMethods.WcsSetUsePerUserProfiles(monitor.MonitorId, MonitorClass, true), "启用当前用户 ICC");
            Check(NativeMethods.WcsAssociateColorProfileWithDevice(CurrentUser, installed, monitor.MonitorId), "关联 ICC");
            SetDefault(monitor.MonitorId, installed);
        }
        WriteRamp(monitor, ramp);
        if (profile.Color.DigitalVibrance is { } value)
        {
            using var nvidia = new NvidiaController();
            nvidia.SetPercent(monitor.DeviceName, value);
        }
    }

    public void Restore(DisplaySnapshot snapshot)
    {
        snapshot.Validate();
        var monitor = Resolve(snapshot.Monitor);
        var errors = new List<Exception>();
        // Attempt every component even if another cannot currently be restored.
        Try(() =>
        {
            if (snapshot.AppliedIccProfiles.Count == 0) return;
            // null removes the per-user default override and returns to the system default.
            SetDefault(monitor.MonitorId, snapshot.UserIccProfile);
            Check(NativeMethods.WcsSetUsePerUserProfiles(monitor.MonitorId, MonitorClass, snapshot.PerUserProfiles), "恢复 ICC 用户配置状态");
        }, errors);
        Try(() =>
        {
            if (AdvancedColorEnabled(monitor)) throw new NotSupportedException("HDR/高级色彩已开启，Gamma 恢复已暂停；请关闭后重试。");
            WriteRamp(monitor, snapshot.GammaRamp);
        }, errors);
        Try(() =>
        {
            if (snapshot.NvidiaVibranceRaw is not { } raw) return;
            using var nvidia = new NvidiaController();
            nvidia.SetRaw(monitor.DeviceName, raw);
        }, errors);
        if (errors.Count > 0) throw new AggregateException("部分色彩状态未能恢复，备份已保留。", errors);
    }

    internal List<string> ListProfiles()
    {
        var system = Path.Combine(Environment.SystemDirectory, "spool", "drivers", "color");
        return new[] { profileDirectory, system }.Where(Directory.Exists)
            .SelectMany(Directory.EnumerateFiles)
            .Where(p => Path.GetExtension(p).Equals(".icc", StringComparison.OrdinalIgnoreCase) || Path.GetExtension(p).Equals(".icm", StringComparison.OrdinalIgnoreCase))
            .Distinct(StringComparer.OrdinalIgnoreCase).OrderBy(Path.GetFileName).ToList();
    }

    internal string ImportProfile(string source)
    {
        IccProfile.Read(source);
        Directory.CreateDirectory(profileDirectory);
        var destination = Path.Combine(profileDirectory, Path.GetFileName(source));
        if (Path.GetFullPath(source).Equals(Path.GetFullPath(destination), StringComparison.OrdinalIgnoreCase)) return destination;
        if (File.Exists(destination))
        {
            if (File.ReadAllBytes(source).SequenceEqual(File.ReadAllBytes(destination))) return destination;
            throw new IOException("存在同名但内容不同的 ICC，请重命名后导入。");
        }
        File.Copy(source, destination);
        return destination;
    }

    private static string EnsureInstalled(string path)
    {
        IccProfile.Read(path);
        var name = Path.GetFileName(path);
        var destination = Path.Combine(Environment.SystemDirectory, "spool", "drivers", "color", name);
        if (File.Exists(destination))
        {
            if (!File.ReadAllBytes(path).SequenceEqual(File.ReadAllBytes(destination)))
                throw new IOException("系统内存在同名但内容不同的 ICC，请重命名后导入。");
        }
        else Check(NativeMethods.InstallColorProfile(0, Path.GetFullPath(path)), "安装 ICC（若系统拒绝，请在 Windows 色彩管理中先安装该文件）");
        return name;
    }

    private static string? DefaultProfile(string monitorId)
    {
        if (!NativeMethods.WcsGetDefaultColorProfileSize(CurrentUser, monitorId, IccType, ProfileSubtype, 0, out var size))
        {
            var error = Marshal.GetLastWin32Error();
            if (error is 2 or 1168 or 2015 or 2016) return null;
            throw Win32("读取默认 ICC 大小", error);
        }
        if (size is 0 or > 65536) throw new InvalidDataException("Windows 返回了无效 ICC 名称长度。");
        var name = new StringBuilder((int)size / 2);
        Check(NativeMethods.WcsGetDefaultColorProfile(CurrentUser, monitorId, IccType, ProfileSubtype, 0, size, name), "读取默认 ICC");
        return name.Length == 0 ? null : name.ToString();
    }

    private static void SetDefault(string monitorId, string? name) =>
        Check(NativeMethods.WcsSetDefaultColorProfile(CurrentUser, monitorId, IccType, ProfileSubtype, 0, name), "设置默认 ICC");

    private static ushort[] ReadRamp(MonitorTarget monitor)
    {
        var dc = NativeMethods.CreateDC("DISPLAY", monitor.DeviceName, 0, 0);
        if (dc == 0) throw Win32("打开显示器设备上下文", Marshal.GetLastWin32Error());
        try
        {
            var ramp = new ushort[768];
            if (!NativeMethods.GetDeviceGammaRamp(dc, ramp)) throw new NotSupportedException("驱动不支持读取 Gamma Ramp。");
            return ramp;
        }
        finally { NativeMethods.DeleteDC(dc); }
    }

    private static void WriteRamp(MonitorTarget monitor, ushort[] ramp)
    {
        var dc = NativeMethods.CreateDC("DISPLAY", monitor.DeviceName, 0, 0);
        if (dc == 0) throw Win32("打开显示器设备上下文", Marshal.GetLastWin32Error());
        try
        {
            if (!NativeMethods.SetDeviceGammaRamp(dc, ramp)) throw new NotSupportedException("驱动拒绝此 Gamma 曲线，请降低调节幅度。");
            var actual = new ushort[768];
            if (!NativeMethods.GetDeviceGammaRamp(dc, actual)) throw new NotSupportedException("无法验证 Gamma 写入。");
            // Some DACs quantize 16-bit values to 8 bits. Larger differences mean silent rejection.
            if (ramp.Where((value, index) => Math.Abs(value - actual[index]) > 257).Any())
                throw new InvalidOperationException("Gamma 写入校验失败：驱动可能静默拒绝或其他应用正在覆盖曲线。");
        }
        finally { NativeMethods.DeleteDC(dc); }
    }

    private static void Try(Action action, List<Exception> errors) { try { action(); } catch (Exception error) { errors.Add(error); } }
    private static void Check(bool result, string operation) { if (!result) throw Win32(operation, Marshal.GetLastWin32Error()); }
    private static void CheckCode(int code, string operation) { if (code != 0) throw Win32(operation, code); }
    private static Exception Win32(string operation, int code) => new Win32Exception(code, $"{operation}失败：{new Win32Exception(code).Message}（{code}）。");
}
