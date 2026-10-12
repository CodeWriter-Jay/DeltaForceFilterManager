namespace DeltaForceFilter.Core;

public sealed record ColorAdjustment
{
    public int Brightness { get; init; }
    public int Contrast { get; init; }
    public double Gamma { get; init; } = 1;
    public int Shadows { get; init; }
    public int Highlights { get; init; }
    public int RedGain { get; init; }
    public int GreenGain { get; init; }
    public int BlueGain { get; init; }
    // null means leave the driver value untouched, including on non-NVIDIA outputs.
    public int? DigitalVibrance { get; init; }

    public void Validate()
    {
        Range(Brightness, -125, 125, "亮度");
        Range(Contrast, -82, 82, "对比度");
        if (!double.IsFinite(Gamma) || Gamma < 0.4 || Gamma > 2.8)
            throw new InvalidDataException("Gamma 必须在 0.4–2.8 之间。");
        Range(Shadows, 0, 50, "暗部提升");
        Range(Highlights, 0, 50, "高光压缩");
        Range(RedGain, -100, 100, "红色增益");
        Range(GreenGain, -100, 100, "绿色增益");
        Range(BlueGain, -100, 100, "蓝色增益");
        if (DigitalVibrance is { } vibrance) Range(vibrance, 0, 100, "数字振动");
    }

    private static void Range(int value, int min, int max, string name)
    {
        if (value < min || value > max) throw new InvalidDataException($"{name}必须在 {min}–{max} 之间。");
    }
}

public sealed record FilterProfile
{
    public string Id { get; init; } = Guid.NewGuid().ToString("N");
    public string Name { get; init; } = "我的方案";
    public string? IccPath { get; init; }
    public ColorAdjustment Color { get; init; } = new();

    public void Validate()
    {
        if (!Guid.TryParseExact(Id, "N", out _) || string.IsNullOrWhiteSpace(Name) || Name.Length > 80)
            throw new InvalidDataException("方案 ID 或名称无效（名称最多 80 个字符）。");
        if (Color is null) throw new InvalidDataException("方案缺少色彩参数。");
        Color.Validate();
        if (IccPath is not null && (IccPath.Length > 1024 || !Path.IsPathFullyQualified(IccPath)))
            throw new InvalidDataException("ICC 路径必须为完整路径。");
    }
}

public sealed record AppSettings
{
    public int SchemaVersion { get; init; } = 1;
    public List<FilterProfile> Profiles { get; init; } = BuiltInProfiles.Create();
    public string? SelectedProfileId { get; init; }
    public string? GameProfileId { get; init; }
    public string? MonitorId { get; init; }
    public bool FollowGame { get; init; }
    public List<string> GameProcesses { get; init; } = ["DeltaForceClient-Win64-Shipping.exe", "DeltaForceClient.exe"];
    public int PollSeconds { get; init; } = 3;
    public bool StartWithWindows { get; init; }
    public bool CloseToTray { get; init; } = true;
    public bool RestoreOnExit { get; init; } = true;

    public void Validate()
    {
        if (SchemaVersion != 1) throw new InvalidDataException("不支持此配置版本。");
        if (Profiles is null || Profiles.Count is < 1 or > 100)
            throw new InvalidDataException("需要 1–100 个方案。");
        foreach (var profile in Profiles)
        {
            if (profile is null) throw new InvalidDataException("方案不能为空。");
            profile.Validate();
        }
        if (Profiles.Select(p => p.Id).Distinct().Count() != Profiles.Count)
            throw new InvalidDataException("方案 ID 重复。");
        if (SelectedProfileId is not null && !Profiles.Any(p => p.Id == SelectedProfileId))
            throw new InvalidDataException("选中的方案不存在。");
        if (GameProfileId is not null && !Profiles.Any(p => p.Id == GameProfileId))
            throw new InvalidDataException("游戏方案不存在。");
        if (PollSeconds is < 2 or > 30) throw new InvalidDataException("进程检测间隔必须为 2–30 秒。");
        if (GameProcesses is null || GameProcesses.Count is < 1 or > 16)
            throw new InvalidDataException("需要 1–16 个游戏进程名。");
        foreach (var process in GameProcesses)
        {
            if (string.IsNullOrWhiteSpace(process) || process.Length > 200 ||
                process.IndexOfAny(['/', '\\', ':', '*', '?', '"', '\0']) >= 0 ||
                !process.EndsWith(".exe", StringComparison.OrdinalIgnoreCase))
                throw new InvalidDataException("请填写进程文件名（例如 game.exe），不要填写路径。");
        }
    }

    public FilterProfile SelectedProfile => Profiles.FirstOrDefault(p => p.Id == SelectedProfileId) ?? Profiles[0];
    public FilterProfile GameProfile => Profiles.FirstOrDefault(p => p.Id == GameProfileId) ?? SelectedProfile;
}

public static class BuiltInProfiles
{
    // Conservative starting points; displays and in-game exposure differ.
    public static List<FilterProfile> Create() =>
    [
        new() { Id = "00000000000000000000000000000001", Name = "均衡 · 三角洲", Color = new() { Gamma = 1.10, Shadows = 14, Highlights = 8 } },
        new() { Id = "00000000000000000000000000000002", Name = "室内 · 暗部可见", Color = new() { Gamma = 1.18, Shadows = 24, Highlights = 10 } },
        new() { Id = "00000000000000000000000000000003", Name = "室外 · 保留高光", Color = new() { Gamma = 1.04, Shadows = 8, Highlights = 24 } },
        new() { Id = "00000000000000000000000000000004", Name = "中性 · 校准基准", Color = new() }
    ];
}

public sealed record MonitorTarget(string DeviceName, string MonitorId, string Label, bool Primary);

public sealed record DisplaySnapshot
{
    public required MonitorTarget Monitor { get; init; }
    public required ushort[] GammaRamp { get; init; }
    public bool PerUserProfiles { get; init; }
    public string? UserIccProfile { get; init; }
    public int? NvidiaVibranceRaw { get; init; }
    // Record intent before each WCS mutation so recovery survives a crash mid-apply.
    public List<string> AppliedIccProfiles { get; init; } = [];

    public void Validate()
    {
        if (Monitor is null || string.IsNullOrWhiteSpace(Monitor.MonitorId) ||
            !Monitor.DeviceName.StartsWith(@"\\.\DISPLAY", StringComparison.OrdinalIgnoreCase) ||
            GammaRamp is null || GammaRamp.Length != 768 || AppliedIccProfiles is null)
            throw new InvalidDataException("恢复备份无效。");
    }
}
