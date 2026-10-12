using System.Buffers.Binary;

namespace DeltaForceFilter.Core;

public sealed record IccData(ushort[]? Calibration, string Description);

public static class IccProfile
{
    public const int MaxFileBytes = 16 * 1024 * 1024;

    public static IccData Read(string path)
    {
        using var stream = File.OpenRead(path);
        if (stream.Length is < 132 or > MaxFileBytes) throw new InvalidDataException("ICC 文件大小无效（最大 16 MB）。");
        var bytes = new byte[(int)stream.Length];
        stream.ReadExactly(bytes);
        return Parse(bytes);
    }

    public static IccData Parse(ReadOnlySpan<byte> bytes)
    {
        if (bytes.Length is < 132 or > MaxFileBytes || !bytes.Slice(36, 4).SequenceEqual("acsp"u8))
            throw new InvalidDataException("文件不是有效的 ICC 配置。");
        var declared = U32(bytes, 0);
        if (declared < 132 || declared > bytes.Length) throw new InvalidDataException("ICC 文件已截断。");
        bytes = bytes[..(int)declared];
        if (!bytes.Slice(12, 4).SequenceEqual("mntr"u8) || !bytes.Slice(16, 4).SequenceEqual("RGB "u8))
            throw new InvalidDataException("只支持 RGB 显示器 ICC 配置（mntr）。");
        var count = U32(bytes, 128);
        if (count > (bytes.Length - 132) / 12) throw new InvalidDataException("ICC 标签表损坏。");
        ushort[]? ramp = null;
        for (var index = 0; index < count; index++)
        {
            var entry = 132 + index * 12;
            var offset = U32(bytes, entry + 4);
            var size = U32(bytes, entry + 8);
            if (offset < 132 + count * 12 || offset > bytes.Length || size > bytes.Length - offset)
                throw new InvalidDataException("ICC 标签超出文件边界。");
            if (!bytes.Slice(entry, 4).SequenceEqual("vcgt"u8)) continue;
            if (ramp is not null) throw new InvalidDataException("ICC 包含重复的 vcgt 标签。");
            ramp = ParseCalibration(bytes.Slice((int)offset, (int)size));
        }
        return new(ramp, ramp is null ? "无 vcgt · 只关联系统色彩配置，游戏曲线使用中性基准" : "含 vcgt · 校准曲线将与调节参数叠加");
    }

    private static ushort[] ParseCalibration(ReadOnlySpan<byte> data)
    {
        if (data.Length < 12 || !data[..4].SequenceEqual("vcgt"u8)) throw new InvalidDataException("vcgt 标签无效。");
        var ramp = new ushort[768];
        switch (U32(data, 8))
        {
            case 0:
                if (data.Length < 18) throw new InvalidDataException("vcgt 表已截断。");
                var channels = U16(data, 12);
                var entries = U16(data, 14);
                var width = U16(data, 16);
                if (channels is not (1 or 3) || entries is < 2 or > 4096 || width is not (1 or 2) ||
                    18 + channels * entries * width > data.Length)
                    throw new InvalidDataException("不支持此 vcgt 表结构，或数据已截断。");
                for (var channel = 0; channel < 3; channel++)
                {
                    var source = channels == 1 ? 0 : channel;
                    for (var index = 0; index < 256; index++)
                    {
                        var position = index * (entries - 1) / 255.0;
                        var lower = (int)position;
                        var upper = Math.Min(lower + 1, entries - 1);
                        var fraction = position - lower;
                        var lo = TableValue(data, 18 + (source * entries + lower) * width, width);
                        var hi = TableValue(data, 18 + (source * entries + upper) * width, width);
                        ramp[channel * 256 + index] = (ushort)Math.Round(lo * (1 - fraction) + hi * fraction);
                    }
                }
                break;
            case 1:
                if (data.Length < 48) throw new InvalidDataException("vcgt 公式已截断。");
                for (var channel = 0; channel < 3; channel++)
                {
                    var offset = 12 + channel * 12;
                    var gamma = U32(data, offset) / 65536.0;
                    var min = U32(data, offset + 4) / 65536.0;
                    var max = U32(data, offset + 8) / 65536.0;
                    if (gamma is <= 0 or > 10 || min > max || max > 1)
                        throw new InvalidDataException("vcgt 公式参数无效。");
                    for (var index = 0; index < 256; index++)
                        ramp[channel * 256 + index] = (ushort)Math.Round((min + (max - min) * Math.Pow(index / 255.0, gamma)) * 65535);
                }
                break;
            default: throw new InvalidDataException("不支持此 vcgt 类型。");
        }
        for (var channel = 0; channel < 3; channel++)
            for (var index = 1; index < 256; index++)
                if (ramp[channel * 256 + index] < ramp[channel * 256 + index - 1])
                    throw new InvalidDataException("vcgt 必须为单调递增曲线。");
        return ramp;
    }

    private static ushort TableValue(ReadOnlySpan<byte> data, int offset, int width) => width == 2 ? U16(data, offset) : (ushort)(data[offset] * 257);
    private static ushort U16(ReadOnlySpan<byte> data, int offset) => BinaryPrimitives.ReadUInt16BigEndian(data.Slice(offset, 2));
    private static uint U32(ReadOnlySpan<byte> data, int offset) => BinaryPrimitives.ReadUInt32BigEndian(data.Slice(offset, 4));
}
