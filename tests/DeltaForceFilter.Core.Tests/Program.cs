using System.Buffers.Binary;
using DeltaForceFilter.Core;

var tests = new (string Name, Action Test)[]
{
    ("Neutral curve preserves calibration exactly", () => Equal(ToneCurve.Identity(), ToneCurve.Build(new()))),
    ("Gamma lifts midtones without changing black", () =>
    {
        var ramp = ToneCurve.Build(new() { Gamma = 1.2 });
        Check(ramp[0] == 0 && ramp[128] > 128 * 257 && ramp[255] == 65535);
    }),
    ("Shadow lift preserves both endpoints", () =>
    {
        var ramp = ToneCurve.Build(new() { Shadows = 50 });
        Check(ramp[0] == 0 && ramp[255] == 65535 && ramp[32] > 32 * 257);
    }),
    ("Highlight compression reduces white output", () =>
    {
        var ramp = ToneCurve.Build(new() { Highlights = 50 });
        Check(ramp[0] == 0 && ramp[255] < 65535 && ramp[64] == 64 * 257);
    }),
    ("Curves remain monotone at all supported extremes", () =>
    {
        foreach (var gamma in new[] { 0.4, 1.0, 2.8 })
        foreach (var brightness in new[] { -125, 0, 125 })
        foreach (var contrast in new[] { -82, 0, 82 })
        foreach (var shadow in new[] { 0, 50 })
        foreach (var highlight in new[] { 0, 50 })
        {
            var ramp = ToneCurve.Build(new() { Gamma = gamma, Brightness = brightness, Contrast = contrast, Shadows = shadow, Highlights = highlight });
            Check(Enumerable.Range(1, 255).All(i => ramp[i] >= ramp[i - 1]));
        }
    }),
    ("RGB gain is channel-local", () =>
    {
        var ramp = ToneCurve.Build(new() { RedGain = -50 });
        Check(ramp[128] < ramp[384] && ramp[384] == ramp[640]);
    }),
    ("Calibration composes without quantization", () =>
    {
        var calibration = ToneCurve.Build(new() { Gamma = 1.1 });
        Equal(calibration, ToneCurve.Build(new(), calibration));
    }),
    ("ICC without vcgt is recognized", () => Check(IccProfile.Parse(MakeIcc(null)).Calibration is null)),
    ("One-channel 8-bit vcgt expands to RGB", () => Equal(ToneCurve.Identity(), IccProfile.Parse(MakeIcc(Table(1, 2, 1))).Calibration!)),
    ("Three-channel 16-bit vcgt interpolates", () => Equal(ToneCurve.Identity(), IccProfile.Parse(MakeIcc(Table(3, 2, 2))).Calibration!)),
    ("vcgt formula uses 32-bit unsigned fixed point", () =>
    {
        var data = new byte[48]; "vcgt"u8.CopyTo(data); Put32(data, 8, 1);
        for (var channel = 0; channel < 3; channel++)
        {
            Put32(data, 12 + channel * 12, 65536); Put32(data, 20 + channel * 12, 65536);
        }
        Equal(ToneCurve.Identity(), IccProfile.Parse(MakeIcc(data)).Calibration!);
    }),
    ("Truncated ICC is rejected", () => Throws<InvalidDataException>(() => IccProfile.Parse(MakeIcc(Table(3, 2, 2))[..^1]))),
    ("Tag escaping its declared size is rejected", () =>
    {
        var data = MakeIcc(Table(3, 2, 2)); Put32(data, 140, 4);
        Throws<InvalidDataException>(() => IccProfile.Parse(data));
    }),
    ("Out-of-bounds tag cannot wrap integer arithmetic", () =>
    {
        var data = MakeIcc(Table(3, 2, 2)); Put32(data, 136, uint.MaxValue);
        Throws<InvalidDataException>(() => IccProfile.Parse(data));
    }),
    ("Non-monotone ICC calibration is rejected", () =>
    {
        var data = Table(1, 2, 1); data[18] = 255; data[19] = 0;
        Throws<InvalidDataException>(() => IccProfile.Parse(MakeIcc(data)));
    }),
    ("Printer and non-RGB profiles are rejected", () =>
    {
        var data = MakeIcc(null); "prtr"u8.CopyTo(data.AsSpan(12));
        Throws<InvalidDataException>(() => IccProfile.Parse(data));
    }),
    ("Invalid color numbers are rejected", () =>
    {
        Throws<InvalidDataException>(() => new ColorAdjustment { Gamma = double.NaN }.Validate());
        Throws<InvalidDataException>(() => new ColorAdjustment { DigitalVibrance = 101 }.Validate());
    }),
    ("Game process settings reject paths and patterns", () =>
    {
        foreach (var name in new[] { "", "*.exe", "C:\\game.exe", "game", "game\0.exe" })
            Throws<InvalidDataException>(() => new AppSettings { GameProcesses = [name] }.Validate());
    }),
    ("Built-in presets and default settings validate", () => new AppSettings().Validate()),
    ("Game already running at app start is detected", () =>
    {
        var state = new GameWatcherState(); Check(state.Observe(true) == GameTransition.Started);
        Check(state.Observe(true) == GameTransition.None); Check(state.Observe(false) == GameTransition.Exited);
    }),
    ("Manual restore is respected until game restart", () =>
    {
        var state = new GameWatcherState(); state.Observe(true); state.SuppressCurrentGame();
        Check(state.Observe(true) == GameTransition.None); state.Observe(false);
        Check(state.Observe(true) == GameTransition.Started);
    }),
    ("Recovery is saved before the first display mutation", () =>
    {
        var backend = new FakeBackend(); var store = new FakeStore();
        backend.BeforeApply = () => Check(store.Snapshot is not null);
        var session = new FilterSession(backend, store); session.Apply(Monitor(), new()); Check(session.IsActive);
    }),
    ("Repeated adjustments do not compound calibration", () =>
    {
        var backend = new FakeBackend(); var session = new FilterSession(backend, new FakeStore());
        var profile = new FilterProfile { Color = new() { Gamma = 1.2 } };
        session.Apply(Monitor(), profile); var first = backend.Ramp!;
        session.Apply(Monitor(), profile); Equal(first, backend.Ramp!); Check(backend.Captures == 1);
    }),
    ("Successful restore clears recovery", () =>
    {
        var store = new FakeStore(); var session = new FilterSession(new FakeBackend(), store);
        session.Apply(Monitor(), new()); session.Restore(); Check(store.Snapshot is null && !session.IsActive && session.Snapshot is null);
    }),
    ("Failed restore retains recovery and session", () =>
    {
        var backend = new FakeBackend(); var store = new FakeStore(); var session = new FilterSession(backend, store);
        session.Apply(Monitor(), new()); backend.FailRestore = true;
        Throws<IOException>(session.Restore); Check(store.Snapshot is not null && session.Snapshot is not null);
    }),
    ("Partial apply failure rolls back", () =>
    {
        var backend = new FakeBackend { FailApply = true }; var store = new FakeStore(); var session = new FilterSession(backend, store);
        Throws<InvalidOperationException>(() => session.Apply(Monitor(), new()));
        Check(backend.Restores == 1 && store.Snapshot is null && !session.IsActive);
    }),
    ("Apply plus rollback failure preserves recovery", () =>
    {
        var backend = new FakeBackend { FailApply = true, FailRestore = true }; var store = new FakeStore(); var session = new FilterSession(backend, store);
        Throws<AggregateException>(() => session.Apply(Monitor(), new())); Check(store.Snapshot is not null);
        backend.FailApply = false;
        Throws<InvalidOperationException>(() => session.Apply(Monitor(), new()));
    }),
    ("Recovery write failure prevents any mutation", () =>
    {
        var backend = new FakeBackend(); var session = new FilterSession(backend, new FakeStore { FailSave = true });
        Throws<IOException>(() => session.Apply(Monitor(), new())); Check(backend.Applies == 0);
    }),
    ("A/B retains original backup and restores the applied profile", () =>
    {
        var backend = new FakeBackend(); var store = new FakeStore(); var session = new FilterSession(backend, store);
        var profile = new FilterProfile { Color = new() { Shadows = 20 } };
        session.Apply(Monitor(), profile); session.CompareOriginal();
        Check(session.IsComparing && !session.IsActive && store.Snapshot is not null);
        session.Apply(Monitor(), session.ActiveProfile!); Check(session.IsActive && !session.IsComparing && backend.Captures == 1);
    }),
    ("Switching monitor cannot overwrite the original backup", () =>
    {
        var session = new FilterSession(new FakeBackend(), new FakeStore()); session.Apply(Monitor(), new());
        Throws<InvalidOperationException>(() => session.Apply(Monitor() with { MonitorId = "other" }, new()));
    }),
    ("NVIDIA is required only when explicitly selected", () =>
    {
        var backend = new FakeBackend(); var session = new FilterSession(backend, new FakeStore());
        session.Apply(Monitor(), new() { Color = new() { DigitalVibrance = 60 } }); Check(backend.RequiredVibrance);
    }),
    ("Malformed recovery is rejected", () => Throws<InvalidDataException>(() => new DisplaySnapshot { Monitor = Monitor(), GammaRamp = [] }.Validate())),
    ("Unknown config fields fail visibly", () =>
    {
        var path = Path.GetTempFileName();
        try { File.WriteAllText(path, "{\"UnexpectedSetting\":true}"); Throws<System.Text.Json.JsonException>(() => Storage.Read<AppSettings>(path)); }
        finally { File.Delete(path); }
    }),
    ("Settings persist with an atomic round trip", () =>
    {
        var directory = Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString("N"));
        try
        {
            var path = Path.Combine(directory, "settings.json"); var settings = new AppSettings { FollowGame = true };
            Storage.Write(path, settings); var read = Storage.Read<AppSettings>(path); read.Validate();
            Check(read.FollowGame && read.Profiles.Count == 4 && Directory.GetFiles(directory).Length == 1);
        }
        finally { if (Directory.Exists(directory)) Directory.Delete(directory, recursive: true); }
    })
};
var failures = 0;
foreach (var (name, test) in tests)
{
    try { test(); Console.WriteLine($"PASS {name}"); }
    catch (Exception error) { failures++; Console.Error.WriteLine($"FAIL {name}: {error}"); }
}
Console.WriteLine($"{tests.Length - failures}/{tests.Length} tests passed.");
return failures == 0 ? 0 : 1;

static void Check(bool result) { if (!result) throw new Exception("Assertion failed."); }
static void Equal(ushort[] first, ushort[] second) => Check(first.SequenceEqual(second));
static void Throws<T>(Action action) where T : Exception
{
    try { action(); } catch (T) { return; }
    throw new Exception($"Expected {typeof(T).Name}.");
}
static MonitorTarget Monitor() => new(@"\\.\DISPLAY1", @"MONITOR\TEST\1", "Test monitor", true);
static void Put32(byte[] data, int offset, uint value) => BinaryPrimitives.WriteUInt32BigEndian(data.AsSpan(offset, 4), value);
static byte[] MakeIcc(byte[]? calibration)
{
    var bytes = new byte[calibration is null ? 132 : 144 + calibration.Length];
    Put32(bytes, 0, (uint)bytes.Length); "mntr"u8.CopyTo(bytes.AsSpan(12)); "RGB "u8.CopyTo(bytes.AsSpan(16)); "acsp"u8.CopyTo(bytes.AsSpan(36));
    if (calibration is not null)
    {
        Put32(bytes, 128, 1); "vcgt"u8.CopyTo(bytes.AsSpan(132)); Put32(bytes, 136, 144); Put32(bytes, 140, (uint)calibration.Length);
        calibration.CopyTo(bytes, 144);
    }
    return bytes;
}
static byte[] Table(int channels, int entries, int width)
{
    var data = new byte[18 + channels * entries * width]; "vcgt"u8.CopyTo(data);
    BinaryPrimitives.WriteUInt16BigEndian(data.AsSpan(12), (ushort)channels);
    BinaryPrimitives.WriteUInt16BigEndian(data.AsSpan(14), (ushort)entries);
    BinaryPrimitives.WriteUInt16BigEndian(data.AsSpan(16), (ushort)width);
    for (var channel = 0; channel < channels; channel++)
        for (var index = 0; index < entries; index++)
        {
            var offset = 18 + (channel * entries + index) * width;
            if (width == 1) data[offset] = (byte)(index * 255 / (entries - 1));
            else BinaryPrimitives.WriteUInt16BigEndian(data.AsSpan(offset), (ushort)(index * 65535 / (entries - 1)));
        }
    return data;
}

sealed class FakeStore : IRecoveryStore
{
    internal DisplaySnapshot? Snapshot;
    internal bool FailSave;
    public void Save(DisplaySnapshot snapshot) { if (FailSave) throw new IOException("disk full"); Snapshot = snapshot; }
    public void Clear() => Snapshot = null;
}
sealed class FakeBackend : IDisplayBackend
{
    internal int Captures, Applies, Restores;
    internal bool FailApply, FailRestore, RequiredVibrance;
    internal Action? BeforeApply;
    internal ushort[]? Ramp;
    public DisplaySnapshot Capture(MonitorTarget monitor, bool requireVibrance)
    {
        Captures++; RequiredVibrance = requireVibrance;
        return new() { Monitor = monitor, GammaRamp = ToneCurve.Identity(), NvidiaVibranceRaw = 50 };
    }
    public void Apply(MonitorTarget monitor, FilterProfile profile, ushort[] ramp)
    {
        BeforeApply?.Invoke(); Applies++; if (FailApply) throw new IOException("driver error"); Ramp = ramp;
    }
    public void Restore(DisplaySnapshot snapshot) { Restores++; if (FailRestore) throw new IOException("monitor disconnected"); Ramp = snapshot.GammaRamp; }
}
