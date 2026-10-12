namespace DeltaForceFilter.Core;

public interface IDisplayBackend
{
    DisplaySnapshot Capture(MonitorTarget monitor, bool requireVibrance);
    void Apply(MonitorTarget monitor, FilterProfile profile, ushort[] ramp);
    void Restore(DisplaySnapshot snapshot);
}

public sealed class FilterSession(IDisplayBackend backend, IRecoveryStore recovery)
{
    public DisplaySnapshot? Snapshot { get; private set; }
    public bool IsActive { get; private set; }
    public bool IsComparing { get; private set; }
    public FilterProfile? ActiveProfile { get; private set; }

    public void AttachRecovery(DisplaySnapshot snapshot)
    {
        if (Snapshot is not null) throw new InvalidOperationException("已有调节会话。");
        snapshot.Validate();
        Snapshot = snapshot;
    }

    public ushort[] Preview(FilterProfile profile)
    {
        profile.Validate();
        var calibration = profile.IccPath is null ? Snapshot?.GammaRamp : IccProfile.Read(profile.IccPath).Calibration ?? ToneCurve.Identity();
        return ToneCurve.Build(profile.Color, calibration);
    }

    public void Apply(MonitorTarget monitor, FilterProfile profile)
    {
        if (Snapshot is not null && !IsActive && !IsComparing)
            throw new InvalidOperationException("存在未完成的恢复备份，请先恢复原始色彩。");
        profile.Validate();
        // Parse external files before touching the monitor or creating a recovery session.
        var icc = profile.IccPath is null ? null : IccProfile.Read(profile.IccPath);
        if (Snapshot is not null && Snapshot.Monitor.MonitorId != monitor.MonitorId)
            throw new InvalidOperationException("请先恢复当前显示器，再切换到另一台显示器。");
        if (Snapshot is not null && profile.Color.DigitalVibrance is not null && Snapshot.NvidiaVibranceRaw is null)
            throw new InvalidOperationException("当前会话没有 NVIDIA 备份，请先恢复后再启用数字振动。");
        var snapshot = Snapshot ?? backend.Capture(monitor, profile.Color.DigitalVibrance is not null);
        snapshot.Validate();
        var profileName = profile.IccPath is null ? null : Path.GetFileName(profile.IccPath);
        if (profileName is not null && !snapshot.AppliedIccProfiles.Contains(profileName, StringComparer.OrdinalIgnoreCase))
            snapshot = snapshot with { AppliedIccProfiles = [.. snapshot.AppliedIccProfiles, profileName] };
        recovery.Save(snapshot);
        Snapshot = snapshot;
        var calibration = profile.IccPath is null ? snapshot.GammaRamp : icc!.Calibration ?? ToneCurve.Identity();
        try
        {
            // Changing to a profile without ICC or DVC restores those components as well.
            if (IsActive || IsComparing) backend.Restore(snapshot);
            backend.Apply(monitor, profile, ToneCurve.Build(profile.Color, calibration));
            IsActive = true;
            IsComparing = false;
            ActiveProfile = profile;
        }
        catch (Exception applyError)
        {
            IsActive = false;
            IsComparing = false;
            ActiveProfile = null;
            try { Restore(); }
            catch (Exception restoreError)
            {
                throw new AggregateException("应用失败且恢复未完成；备份已保留，请点击恢复原始色彩。", applyError, restoreError);
            }
            throw new InvalidOperationException($"应用失败，已恢复原始色彩：{applyError.Message}", applyError);
        }
    }

    public void CompareOriginal()
    {
        if (!IsActive || Snapshot is null) return;
        backend.Restore(Snapshot);
        IsActive = false;
        IsComparing = true;
    }

    public void Restore()
    {
        if (Snapshot is null) return;
        backend.Restore(Snapshot);
        recovery.Clear();
        Snapshot = null;
        ActiveProfile = null;
        IsActive = false;
        IsComparing = false;
    }
}

public enum GameTransition { None, Started, Exited }

public sealed class GameWatcherState
{
    private bool running;
    private bool suppressed;

    public GameTransition Observe(bool gameRunning)
    {
        if (!gameRunning)
        {
            var exited = running;
            running = false;
            suppressed = false;
            return exited ? GameTransition.Exited : GameTransition.None;
        }
        if (running) return GameTransition.None;
        running = true;
        return suppressed ? GameTransition.None : GameTransition.Started;
    }

    // A manual restore during the same game session must not be undone by polling.
    public void SuppressCurrentGame() => suppressed = true;
    public void Reset() { running = false; suppressed = false; }
}
