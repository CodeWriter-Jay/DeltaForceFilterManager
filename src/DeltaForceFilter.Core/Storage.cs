using System.Text.Json;
using System.Text.Json.Serialization;

namespace DeltaForceFilter.Core;

public static class Storage
{
    private static readonly JsonSerializerOptions Options = new()
    {
        WriteIndented = true,
        PropertyNameCaseInsensitive = false,
        UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow
    };

    public static T Read<T>(string path, int maxBytes = 1024 * 1024)
    {
        using var stream = File.OpenRead(path);
        if (stream.Length > maxBytes) throw new InvalidDataException("配置文件过大。");
        return JsonSerializer.Deserialize<T>(stream, Options) ?? throw new InvalidDataException("配置文件为空。");
    }

    public static void Write<T>(string path, T value)
    {
        var directory = Path.GetDirectoryName(Path.GetFullPath(path))!;
        Directory.CreateDirectory(directory);
        var temp = Path.Combine(directory, $".{Path.GetFileName(path)}.{Guid.NewGuid():N}.tmp");
        try
        {
            using (var stream = new FileStream(temp, FileMode.CreateNew, FileAccess.Write, FileShare.None))
            {
                JsonSerializer.Serialize(stream, value, Options);
                stream.Flush(flushToDisk: true);
            }
            File.Move(temp, path, overwrite: true);
        }
        finally
        {
            if (File.Exists(temp)) File.Delete(temp);
        }
    }
}

public interface IRecoveryStore
{
    void Save(DisplaySnapshot snapshot);
    void Clear();
}

public sealed class FileRecoveryStore(string path) : IRecoveryStore
{
    public void Save(DisplaySnapshot snapshot) => Storage.Write(path, snapshot);
    public void Clear() => File.Delete(path);
}
