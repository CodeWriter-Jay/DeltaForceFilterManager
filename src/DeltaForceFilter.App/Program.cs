using DeltaForceFilter.App.Platform;
using DeltaForceFilter.App.UI;
using DeltaForceFilter.Core;

namespace DeltaForceFilter.App;

internal static class Program
{
    [STAThread]
    private static int Main(string[] args)
    {
        ApplicationConfiguration.Initialize();
        using var singleInstance = new Mutex(true, @"Local\DeltaForceFilterManager", out var created);
        if (!created)
        {
            MessageBox.Show("程序已在运行。请双击通知区域中的图标打开窗口。", "DeltaForce Filter", MessageBoxButtons.OK, MessageBoxIcon.Information);
            return 1;
        }
        var smoke = args.Contains("--ui-smoke-test", StringComparer.Ordinal);
        var dataDirectory = smoke ? Path.Combine(Path.GetTempPath(), "DeltaForceFilterSmoke", Guid.NewGuid().ToString("N"))
            : Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "DeltaForceFilterManager");
        Directory.CreateDirectory(dataDirectory);
        var configPath = Path.Combine(dataDirectory, "settings.json");
        var recoveryPath = Path.Combine(dataDirectory, "recovery.json");
        try
        {
            var settings = File.Exists(configPath) ? Storage.Read<AppSettings>(configPath) : new AppSettings();
            settings.Validate();
            var backend = new WindowsDisplayBackend(Path.Combine(dataDirectory, "icc"));
            var session = new FilterSession(backend, new FileRecoveryStore(recoveryPath));
            string? recoveryError = null;
            if (File.Exists(recoveryPath))
            {
                session.AttachRecovery(Storage.Read<DisplaySnapshot>(recoveryPath));
                try { session.Restore(); }
                catch (Exception error) { recoveryError = ErrorText(error); }
            }
            if (args.Contains("--restore", StringComparer.Ordinal))
            {
                if (recoveryError is not null) throw new InvalidOperationException(recoveryError);
                return 0;
            }
            using var form = new MainForm(settings, configPath, backend, session, args.Contains("--tray", StringComparer.Ordinal), smoke);
            if (recoveryError is not null) form.SetError($"上次异常退出的备份尚未恢复：{recoveryError}");
            if (smoke)
            {
                var output = args.SkipWhile(a => a != "--ui-smoke-test").Skip(1).FirstOrDefault()
                    ?? Path.Combine(Environment.CurrentDirectory, "artifacts", "ui.png");
                form.Show();
                form.Refresh();
                Application.DoEvents();
                Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(output))!);
                using var bitmap = new Bitmap(form.Width, form.Height);
                form.DrawToBitmap(bitmap, new(0, 0, bitmap.Width, bitmap.Height));
                bitmap.Save(output);
                form.CloseForSmokeTest();
                return 0;
            }
            Application.Run(form);
            return 0;
        }
        catch (Exception error)
        {
            if (smoke)
            {
                File.WriteAllText(Path.Combine(Environment.CurrentDirectory, "smoke-error.txt"), error.ToString());
                return 2;
            }
            MessageBox.Show($"{ErrorText(error)}\n\n配置目录：{dataDirectory}\n原文件已保留。", "DeltaForce Filter · 启动失败", MessageBoxButtons.OK, MessageBoxIcon.Error);
            return 2;
        }
    }

    internal static string ErrorText(Exception error) => error is AggregateException aggregate
        ? string.Join("\n", aggregate.Flatten().InnerExceptions.Select(e => e.Message)) : error.Message;
}
