using System.Runtime.InteropServices;

namespace DeltaForceFilter.App.Platform;

internal sealed class NvidiaController : IDisposable
{
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate nint Query(uint id);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int Initialize();
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int Associate([MarshalAs(UnmanagedType.LPStr)] string name, out nint handle);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int GetDvc(nint handle, uint output, ref DvcInfo info);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int SetDvc(nint handle, uint output, ref DvcInfo info);
    [StructLayout(LayoutKind.Sequential)]
    private struct DvcInfo { internal uint Version; internal int Current, Min, Max, Default; }

    private readonly nint library;
    private readonly Associate associate;
    private readonly GetDvc get;
    private readonly SetDvc set;

    internal NvidiaController()
    {
        // Always load the installed driver from System32, never from the working directory.
        library = NativeLibrary.Load(Path.Combine(Environment.SystemDirectory, "nvapi64.dll"));
        try
        {
            var query = Marshal.GetDelegateForFunctionPointer<Query>(NativeLibrary.GetExport(library, "nvapi_QueryInterface"));
            Check(Function<Initialize>(query, 0x0150E828)(), "初始化 NVAPI");
            associate = Function<Associate>(query, 0x35C29134);
            get = Function<GetDvc>(query, 0x0E45002D);
            set = Function<SetDvc>(query, 0x4A82C2B1);
        }
        catch { NativeLibrary.Free(library); throw; }
    }

    private static T Function<T>(Query query, uint id) where T : Delegate
    {
        var address = query(id);
        if (address == 0) throw new NotSupportedException($"驱动未提供 NVAPI 入口 0x{id:X8}。");
        return Marshal.GetDelegateForFunctionPointer<T>(address);
    }

    private nint Handle(string display)
    {
        Check(associate(display, out var handle), $"匹配 NVIDIA 显示器 {display}");
        if (handle == 0) throw new NotSupportedException("所选显示器没有 NVIDIA 输出句柄。");
        return handle;
    }

    private DvcInfo Read(nint handle)
    {
        var info = new DvcInfo { Version = (uint)Marshal.SizeOf<DvcInfo>() | 0x10000 };
        Check(get(handle, 0, ref info), "读取数字振动");
        if (info.Max <= info.Min || info.Current < info.Min || info.Current > info.Max)
            throw new InvalidDataException("NVIDIA 返回的数字振动范围无效。");
        return info;
    }

    internal int ReadRaw(string display) => Read(Handle(display)).Current;
    internal int ReadPercent(string display)
    {
        var info = Read(Handle(display));
        return (int)Math.Round((info.Current - info.Min) * 100.0 / (info.Max - info.Min));
    }

    internal void SetPercent(string display, int percent)
    {
        if (percent is < 0 or > 100) throw new ArgumentOutOfRangeException(nameof(percent));
        var handle = Handle(display);
        var info = Read(handle);
        Write(handle, info, info.Min + (int)Math.Round(percent / 100.0 * (info.Max - info.Min)));
    }

    internal void SetRaw(string display, int raw)
    {
        var handle = Handle(display);
        var info = Read(handle);
        if (raw < info.Min || raw > info.Max) throw new InvalidOperationException("驱动数字振动范围已改变，无法恢复旧值。");
        Write(handle, info, raw);
    }

    private void Write(nint handle, DvcInfo info, int level)
    {
        info.Current = level;
        Check(set(handle, 0, ref info), "设置数字振动");
        if (Read(handle).Current != level) throw new InvalidOperationException("NVIDIA 数字振动写入后校验失败。");
    }

    private static void Check(int status, string operation)
    {
        if (status != 0) throw new NotSupportedException($"{operation}失败（NVAPI {status}）。请检查显示器连接和驱动版本。");
    }

    public void Dispose() => NativeLibrary.Free(library);
}
