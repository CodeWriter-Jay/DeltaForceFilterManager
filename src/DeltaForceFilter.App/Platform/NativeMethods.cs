using System.Runtime.InteropServices;
using System.Text;

namespace DeltaForceFilter.App.Platform;

internal static class NativeMethods
{
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    internal struct DisplayDevice
    {
        internal uint Size;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] internal string Name;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] internal string Description;
        internal uint Flags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] internal string Id;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] internal string Key;
        internal static DisplayDevice Create() => new() { Size = (uint)Marshal.SizeOf<DisplayDevice>(), Name = "", Description = "", Id = "", Key = "" };
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool EnumDisplayDevices(string? device, uint index, ref DisplayDevice display, uint flags);

    [DllImport("gdi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern nint CreateDC(string driver, string device, nint output, nint initData);
    [DllImport("gdi32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool DeleteDC(nint dc);
    [DllImport("gdi32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool GetDeviceGammaRamp(nint dc, [Out] ushort[] ramp);
    [DllImport("gdi32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool SetDeviceGammaRamp(nint dc, [In] ushort[] ramp);

    [DllImport("mscms.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WcsGetUsePerUserProfiles(string device, uint deviceClass, [MarshalAs(UnmanagedType.Bool)] out bool enabled);
    [DllImport("mscms.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WcsSetUsePerUserProfiles(string device, uint deviceClass, [MarshalAs(UnmanagedType.Bool)] bool enabled);
    [DllImport("mscms.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WcsGetDefaultColorProfileSize(uint scope, string device, uint type, uint subtype, uint profileId, out uint size);
    [DllImport("mscms.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WcsGetDefaultColorProfile(uint scope, string device, uint type, uint subtype, uint profileId, uint size, StringBuilder name);
    [DllImport("mscms.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WcsSetDefaultColorProfile(uint scope, string device, uint type, uint subtype, uint profileId, string? name);
    [DllImport("mscms.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WcsAssociateColorProfileWithDevice(uint scope, string profile, string device);
    [DllImport("mscms.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool InstallColorProfile(nint machine, string path);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool RegisterHotKey(nint window, int id, uint modifiers, uint key);
    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool UnregisterHotKey(nint window, int id);

    [DllImport("dwmapi.dll")]
    internal static extern int DwmSetWindowAttribute(nint window, uint attribute, ref int value, uint size);

    [StructLayout(LayoutKind.Sequential)]
    internal struct Luid { internal uint Low; internal int High; }
    [StructLayout(LayoutKind.Sequential)]
    internal struct PathSource { internal Luid Adapter; internal uint Id, Mode, Flags; }
    [StructLayout(LayoutKind.Sequential)]
    internal struct PathTarget
    {
        internal Luid Adapter;
        internal uint Id, Mode, Technology, Rotation, Scaling, RefreshNumerator, RefreshDenominator, Scanline;
        internal int Available;
        internal uint Flags;
    }
    [StructLayout(LayoutKind.Sequential)]
    internal struct DisplayPath { internal PathSource Source; internal PathTarget Target; internal uint Flags; }
    [StructLayout(LayoutKind.Explicit, Size = 64)]
    internal struct DisplayMode { [FieldOffset(0)] internal uint Type; }
    [StructLayout(LayoutKind.Sequential)]
    internal struct InfoHeader { internal uint Type, Size; internal Luid Adapter; internal uint Id; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    internal struct SourceName
    {
        internal InfoHeader Header;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] internal string Name;
    }
    [StructLayout(LayoutKind.Sequential)]
    internal struct AdvancedColor { internal InfoHeader Header; internal uint Value, Encoding, BitsPerChannel; }

    [DllImport("user32.dll")]
    internal static extern int GetDisplayConfigBufferSizes(uint flags, out uint paths, out uint modes);
    [DllImport("user32.dll")]
    internal static extern int QueryDisplayConfig(uint flags, ref uint paths, [Out] DisplayPath[] pathArray, ref uint modes, [Out] DisplayMode[] modeArray, nint topology);
    [DllImport("user32.dll", EntryPoint = "DisplayConfigGetDeviceInfo")]
    internal static extern int GetSourceName(ref SourceName info);
    [DllImport("user32.dll", EntryPoint = "DisplayConfigGetDeviceInfo")]
    internal static extern int GetAdvancedColor(ref AdvancedColor info);
}
