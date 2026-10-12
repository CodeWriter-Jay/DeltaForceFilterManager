using System.Diagnostics;
using System.Runtime.InteropServices;
using DeltaForceFilter.App.Platform;
using DeltaForceFilter.Core;

namespace DeltaForceFilter.App.UI;

internal sealed class MainForm : Form
{
    private AppSettings settings;
    private readonly string configPath;
    private readonly WindowsDisplayBackend backend;
    private readonly FilterSession session;
    private readonly bool startInTray;
    private readonly bool smoke;
    private readonly GameWatcherState watcher = new();
    private readonly System.Windows.Forms.Timer processTimer = new();
    private readonly System.Windows.Forms.Timer previewTimer = new() { Interval = 180 };
    private readonly NotifyIcon tray;
    private readonly ComboBox monitors = Theme.Combo();
    private readonly ComboBox profiles = Theme.Combo();
    private readonly ComboBox icc = Theme.Combo();
    private readonly ImagePreview preview = new() { Dock = DockStyle.Fill, Margin = new(0) };
    private readonly CurveView curve = new() { Dock = DockStyle.Fill };
    private readonly Label status = Theme.Label("滤镜未开启", 9.5f, Theme.Muted);
    private readonly Label driver = Theme.Label("", 8, Theme.Muted);
    private readonly Label imageName = Theme.Label("内置灰阶与色卡", 9, Theme.Muted);
    private readonly Label statusDetail = Theme.Label("", 9, Theme.Muted);
    private readonly Dictionary<string, ParameterControl> parameters = new();
    private readonly CheckBox vibranceEnabled = Theme.Check("启用 NVIDIA 数字振动", false);
    private readonly CheckBox follow = Theme.Check("跟随游戏进程自动开启", false);
    private readonly Panel tuningPage = new() { Dock = DockStyle.Fill };
    private readonly Panel settingsPage = new() { Dock = DockStyle.Fill, Visible = false };
    private readonly ToolTip toolTip = new() { AutoPopDelay = 12000 };
    private FilterProfile draft;
    private List<MonitorTarget> targets = [];
    private string? imagePath;
    private bool loading;
    private bool quitting;
    private bool busy;
    private bool autoOwned;
    private bool suspendedPreview;
    private bool recoveryPending;
    private int watcherGeneration;
    private Action? refreshGameProfiles;

    internal MainForm(AppSettings settings, string configPath, WindowsDisplayBackend backend, FilterSession session, bool startInTray, bool smoke)
    {
        this.settings = settings;
        this.configPath = configPath;
        this.backend = backend;
        this.session = session;
        this.startInTray = startInTray;
        this.smoke = smoke;
        draft = settings.SelectedProfile;
        recoveryPending = session.Snapshot is not null;
        Text = "DeltaForce Filter · 显示器色彩工作台";
        BackColor = Theme.Background;
        ForeColor = Theme.Text;
        Font = Theme.Font();
        AutoScaleMode = AutoScaleMode.Dpi;
        ClientSize = new(1360, 900);
        MinimumSize = new(1120, 780);
        StartPosition = FormStartPosition.CenterScreen;
        Icon = SystemIcons.Application;
        BuildLayout();
        BuildSettings();
        tray = new NotifyIcon { Icon = Icon, Text = "DeltaForce Filter · 未开启", Visible = !smoke };
        var menu = new ContextMenuStrip();
        menu.Items.Add("打开色彩工作台", null, (_, _) => OpenWindow());
        menu.Items.Add("手动开启 / 关闭", null, (_, _) => ToggleFilter());
        menu.Items.Add("恢复原始色彩", null, (_, _) => Restore(manual: true));
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add("退出", null, (_, _) => Quit());
        tray.ContextMenuStrip = menu;
        tray.DoubleClick += (_, _) => OpenWindow();
        previewTimer.Tick += (_, _) => { previewTimer.Stop(); Run(UpdatePreview); };
        processTimer.Tick += async (_, _) => await CheckGame();
        FormClosing += HandleFormClosing;
        FormClosed += (_, _) => DisposeResources();
        Resize += (_, _) => { if (!smoke && IsHandleCreated && WindowState == FormWindowState.Minimized) HideToTray(); };
        Shown += async (_, _) =>
        {
            if (smoke) return;
            if (!NativeMethods.RegisterHotKey(Handle, 1, 0x4000 | 2 | 4, (uint)Keys.F10))
                SetError("Ctrl+Shift+F10 已被占用，手动开关快捷键不可用。");
            if (!NativeMethods.RegisterHotKey(Handle, 2, 0x4000 | 2 | 4, (uint)Keys.F11))
                SetError("Ctrl+Shift+F11 已被占用，恢复快捷键不可用。");
            if (startInTray && !recoveryPending) HideToTray();
            ConfigureWatcher();
            if (settings.FollowGame && !recoveryPending) await CheckGame();
        };
        RefreshProfiles();
        RefreshDisplays();
        RefreshIcc();
        LoadDraft(draft);
        UpdateStatus();
    }

    private void BuildLayout()
    {
        var root = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, RowCount = 1, Padding = new(0) };
        root.ColumnStyles.Add(new(SizeType.Absolute, 218));
        root.ColumnStyles.Add(new(SizeType.Percent, 100));
        Controls.Add(root);
        var sidebar = new Panel { Dock = DockStyle.Fill, BackColor = Theme.Surface, Padding = new(22, 28, 16, 20), Margin = new(0) };
        var sidebarStack = new FlowLayoutPanel { Dock = DockStyle.Fill, FlowDirection = FlowDirection.TopDown, WrapContents = false };
        sidebarStack.Controls.Add(Theme.Label("◈  DELTA", 23, Theme.Accent, FontStyle.Bold));
        sidebarStack.Controls.Add(Theme.Label("F I L T E R   S T U D I O", 8, Theme.Muted));
        sidebarStack.Controls.Add(Spacer(20));
        var tune = Theme.Button("◉   色彩工作台", (_, _) => ShowPage(false), true);
        tune.Width = 173; tune.AutoSize = false;
        var options = Theme.Button("⚙   应用设置", (_, _) => ShowPage(true));
        options.Width = 173; options.AutoSize = false;
        sidebarStack.Controls.Add(tune);
        sidebarStack.Controls.Add(options);
        sidebarStack.Controls.Add(Spacer(24));
        sidebarStack.Controls.Add(Theme.Label("当前方案", 8, Theme.Muted));
        profiles.Width = 173; profiles.Dock = DockStyle.None;
        profiles.DisplayMember = nameof(FilterProfile.Name);
        profiles.AccessibleName = "当前滤镜方案";
        profiles.SelectedIndexChanged += (_, _) => { if (!loading && profiles.SelectedItem is FilterProfile selected) Run(() => SelectProfile(selected)); };
        sidebarStack.Controls.Add(profiles);
        var save = Theme.Button("保存当前方案", (_, _) => Run(SaveDraft));
        save.Width = 173; save.AutoSize = false;
        sidebarStack.Controls.Add(save);
        var duplicate = Theme.Button("另存新方案", (_, _) => Run(DuplicateProfile));
        duplicate.Width = 173; duplicate.AutoSize = false;
        sidebarStack.Controls.Add(duplicate);
        sidebarStack.Controls.Add(Spacer(12));
        sidebarStack.Controls.Add(follow);
        follow.MaximumSize = new(175, 0);
        follow.Checked = settings.FollowGame;
        follow.CheckedChanged += (_, _) =>
        {
            if (loading) return;
            Run(() =>
            {
                var next = settings with { FollowGame = follow.Checked };
                SaveSettings(next);
                if (!settings.FollowGame && autoOwned) Restore(manual: false);
                ConfigureWatcher();
            });
        };
        sidebarStack.Controls.Add(Spacer(24));
        var help = Theme.Label("Ctrl + Shift + F10\n开启 / 关闭滤镜\n\nCtrl + Shift + F11\n恢复原始色彩", 8, Theme.Muted);
        help.MaximumSize = new(175, 0);
        sidebarStack.Controls.Add(help);
        sidebar.Controls.Add(sidebarStack);
        root.Controls.Add(sidebar, 0, 0);
        var main = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 3, Padding = new(26, 22, 26, 18) };
        main.RowStyles.Add(new(SizeType.Absolute, 100));
        main.RowStyles.Add(new(SizeType.Percent, 100));
        main.RowStyles.Add(new(SizeType.Absolute, 68));
        root.Controls.Add(main, 1, 0);
        var header = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, RowCount = 2 };
        header.ColumnStyles.Add(new(SizeType.Percent, 100));
        header.ColumnStyles.Add(new(SizeType.Absolute, 190));
        header.Controls.Add(Theme.Label("让每一处细节，都清晰可见。", 20, Theme.Text, FontStyle.Bold), 0, 0);
        header.Controls.Add(Theme.Label("三角洲行动  /  显示器色彩与明暗调节", 9, Theme.Muted), 0, 1);
        status.TextAlign = ContentAlignment.MiddleRight;
        status.Dock = DockStyle.Fill;
        header.Controls.Add(status, 1, 0);
        var badge = Theme.Label("SDR  ·  原生桌面应用", 8, Theme.Accent);
        header.Controls.Add(badge, 1, 1);
        main.Controls.Add(header, 0, 0);
        var pages = new Panel { Dock = DockStyle.Fill };
        pages.Controls.Add(tuningPage); pages.Controls.Add(settingsPage);
        main.Controls.Add(pages, 0, 1);
        BuildTuning();
        statusDetail.Dock = DockStyle.Fill;
        statusDetail.AutoSize = false;
        statusDetail.Text = "选择显示器和方案，先用参考图片调节，再点击“开启滤镜”。";
        main.Controls.Add(statusDetail, 0, 2);
    }

    private void BuildTuning()
    {
        var columns = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, RowCount = 1 };
        columns.ColumnStyles.Add(new(SizeType.Percent, 100));
        columns.ColumnStyles.Add(new(SizeType.Absolute, 320));
        tuningPage.Controls.Add(columns);
        var left = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 3, Margin = new(0, 0, 16, 0) };
        left.RowStyles.Add(new(SizeType.Percent, 100));
        left.RowStyles.Add(new(SizeType.Absolute, 176));
        left.RowStyles.Add(new(SizeType.Absolute, 76));
        columns.Controls.Add(left, 0, 0);
        var imageCard = new Card { Dock = DockStyle.Fill, Margin = new(0, 0, 0, 14) };
        var imageLayout = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 4 };
        imageLayout.RowStyles.Add(new(SizeType.Absolute, 30));
        imageLayout.RowStyles.Add(new(SizeType.Absolute, 44));
        imageLayout.RowStyles.Add(new(SizeType.Percent, 100));
        imageLayout.RowStyles.Add(new(SizeType.Absolute, 40));
        imageLayout.Controls.Add(Theme.Label("REFERENCE  /  图片对照", 10, Theme.Text, FontStyle.Bold), 0, 0);
        var toolbar = new FlowLayoutPanel { Dock = DockStyle.Fill, WrapContents = false };
        toolbar.Controls.Add(Theme.Button("导入参考图片", (_, _) => Run(ImportImage), true));
        toolbar.Controls.Add(Theme.Button("重置色卡", (_, _) => Run(() => { imagePath = null; preview.ResetImage(); imageName.Text = "内置灰阶与色卡"; UpdatePreview(); })));
        var compare = Theme.Check("分屏", true); compare.Margin = new(4, 4, 0, 0);
        compare.CheckedChanged += (_, _) => preview.Comparing = compare.Checked;
        toolbar.Controls.Add(compare);
        imageLayout.Controls.Add(toolbar, 0, 1);
        imageLayout.Controls.Add(preview, 0, 2);
        imageName.Dock = DockStyle.Fill; imageName.AutoSize = false;
        imageLayout.Controls.Add(imageName, 0, 3);
        imageCard.Controls.Add(imageLayout); left.Controls.Add(imageCard, 0, 0);
        preview.AllowDrop = true;
        preview.DragEnter += (_, e) => e.Effect = e.Data?.GetDataPresent(DataFormats.FileDrop) == true ? DragDropEffects.Copy : DragDropEffects.None;
        preview.DragDrop += (_, e) => { if (e.Data?.GetData(DataFormats.FileDrop) is string[] { Length: > 0 } files) Run(() => LoadImage(files[0])); };
        var curveCard = new Card { Dock = DockStyle.Fill, Margin = new(0, 0, 0, 14), Padding = new(12) };
        var curveLayout = new TableLayoutPanel { Dock = DockStyle.Fill, RowCount = 2, ColumnCount = 1 };
        curveLayout.RowStyles.Add(new(SizeType.Absolute, 26)); curveLayout.RowStyles.Add(new(SizeType.Percent, 100));
        curveLayout.Controls.Add(Theme.Label("TONE CURVE  /  RGB 曲线", 8, Theme.Muted), 0, 0);
        curveLayout.Controls.Add(curve, 0, 1); curveCard.Controls.Add(curveLayout); left.Controls.Add(curveCard, 0, 1);
        var actions = new FlowLayoutPanel { Dock = DockStyle.Fill, WrapContents = true };
        actions.Controls.Add(Theme.Button("开启 / 应用滤镜", (_, _) => ApplyDraft(), true));
        actions.Controls.Add(Theme.Button("A/B 原始对照", (_, _) => CompareOriginal()));
        actions.Controls.Add(Theme.Button("恢复原始色彩", (_, _) => Restore(manual: true)));
        toolTip.SetToolTip(actions.Controls[1], "临时恢复整个显示器。再次点击返回当前方案；结束对照请点击恢复原始色彩。");
        left.Controls.Add(actions, 0, 2);

        var adjustCard = new Card { Dock = DockStyle.Fill, Margin = new(0), Padding = new(16) };
        columns.Controls.Add(adjustCard, 1, 0);
        var scroll = new Panel { Dock = DockStyle.Fill, AutoScroll = true };
        var stack = new TableLayoutPanel { Dock = DockStyle.Top, ColumnCount = 1, AutoSize = true, AutoSizeMode = AutoSizeMode.GrowAndShrink, Padding = new(0, 0, 12, 0) };
        scroll.Controls.Add(stack); adjustCard.Controls.Add(scroll);
        AddRow(stack, Theme.Label("DISPLAY  /  目标显示器", 9, Theme.Text, FontStyle.Bold));
        monitors.DisplayMember = nameof(MonitorTarget.Label);
        monitors.AccessibleName = "目标显示器";
        monitors.SelectedIndexChanged += (_, _) => { if (!loading) Run(SelectMonitor); };
        AddRow(stack, monitors);
        driver.MaximumSize = new(270, 0); AddRow(stack, driver);
        AddRow(stack, Theme.Label("ICC 色彩配置", 9, Theme.Muted));
        icc.DisplayMember = nameof(IccChoice.Name);
        icc.AccessibleName = "ICC 色彩配置";
        icc.SelectedIndexChanged += (_, _) =>
        {
            if (loading || icc.SelectedItem is not IccChoice selected) return;
            Run(() => { draft = draft with { IccPath = selected.Path }; SchedulePreview(); });
        };
        AddRow(stack, icc);
        var iccActions = new FlowLayoutPanel { Dock = DockStyle.Top, AutoSize = true, WrapContents = true };
        iccActions.Controls.Add(Theme.Button("导入 ICC", (_, _) => Run(ImportIcc)));
        iccActions.Controls.Add(Theme.Button("导出 ICC", (_, _) => Run(ExportIcc)));
        AddRow(stack, iccActions);
        AddRow(stack, Theme.Label("COLOR  /  色彩调节", 9, Theme.Accent, FontStyle.Bold));
        Parameter(stack, "brightness", "亮度", -125, 125, 1);
        Parameter(stack, "contrast", "对比度", -82, 82, 1);
        Parameter(stack, "gamma", "Gamma", 0.4m, 2.8m, 0.01m, "大于 1 可提亮中间调");
        Parameter(stack, "shadows", "暗部提升", 0, 50, 1, "改善室内暗部，保留黑色基点");
        Parameter(stack, "highlights", "高光压缩", 0, 50, 1, "降低亮部输出，无法恢复已曝光丢失的细节");
        AddRow(stack, Theme.Label("RGB  /  通道增益", 9, Theme.Muted, FontStyle.Bold));
        Parameter(stack, "red", "红色", -100, 100, 1);
        Parameter(stack, "green", "绿色", -100, 100, 1);
        Parameter(stack, "blue", "蓝色", -100, 100, 1);
        AddRow(stack, vibranceEnabled);
        Parameter(stack, "vibrance", "数字振动", 0, 100, 1, "直接作用于 NVIDIA 输出；图片不模拟此参数");
        vibranceEnabled.CheckedChanged += (_, _) => { if (!loading) { parameters["vibrance"].Enabled = vibranceEnabled.Checked; DraftChanged(); } };
        AddRow(stack, Theme.Button("重置调节参数", (_, _) => Run(() => LoadDraft(draft with { Color = new() }))));
    }

    private void BuildSettings()
    {
        var scroll = new Panel { Dock = DockStyle.Fill, AutoScroll = true };
        settingsPage.Controls.Add(scroll);
        var card = new Card { Dock = DockStyle.Top, AutoSize = true, Padding = new(24) };
        var stack = new TableLayoutPanel { Dock = DockStyle.Top, AutoSize = true, ColumnCount = 1 };
        card.Controls.Add(stack); scroll.Controls.Add(card);
        AddRow(stack, Theme.Label("按你的习惯运行", 18, Theme.Text, FontStyle.Bold));
        AddRow(stack, Theme.Label("设置只保存到本机；参考图片不会上传到网络。", 9, Theme.Muted));
        var startup = Theme.Check("随 Windows 登录启动（进入托盘）", settings.StartWithWindows);
        var close = Theme.Check("关闭窗口时最小化到托盘", settings.CloseToTray);
        var restore = Theme.Check("退出应用时恢复原始色彩", settings.RestoreOnExit);
        AddRow(stack, startup); AddRow(stack, close); AddRow(stack, restore);
        AddRow(stack, Theme.Label("游戏进程联动", 13, Theme.Accent, FontStyle.Bold));
        AddRow(stack, Theme.Label("进程存在时应用指定方案，最后一个匹配进程退出后恢复。", 9, Theme.Muted));
        var gameProfile = Theme.Combo(); gameProfile.DisplayMember = nameof(FilterProfile.Name);
        void PopulateGameProfiles()
        {
            gameProfile.DataSource = settings.Profiles.ToList();
            gameProfile.SelectedItem = settings.GameProfile;
        }
        refreshGameProfiles = PopulateGameProfiles;
        PopulateGameProfiles();
        AddRow(stack, Theme.Label("自动应用方案", 9, Theme.Muted)); AddRow(stack, gameProfile);
        var processes = Theme.TextBox(string.Join(Environment.NewLine, settings.GameProcesses));
        processes.AccessibleName = "游戏进程名，每行一个 exe 文件名";
        processes.Multiline = true; processes.Height = 74;
        AddRow(stack, Theme.Label("游戏进程名（每行一个 .exe，可从正在运行的进程中选择）", 9, Theme.Muted));
        AddRow(stack, processes);
        AddRow(stack, Theme.Button("选择运行中的进程", (_, _) => Run(() =>
        {
            var names = Process.GetProcesses().Select(p =>
            {
                using (p) { try { return p.ProcessName + ".exe"; } catch (InvalidOperationException) { return null; } }
            }).Where(n => n is not null).Distinct(StringComparer.OrdinalIgnoreCase).OrderBy(n => n).ToArray();
            using var dialog = new Form { Text = "选择游戏进程", ClientSize = new(520, 520), StartPosition = FormStartPosition.CenterParent, BackColor = Theme.Surface, ForeColor = Theme.Text, Font = Theme.Font(), MinimizeBox = false, MaximizeBox = false };
            var list = new ListBox { Dock = DockStyle.Fill, BackColor = Theme.Raised, ForeColor = Theme.Text, SelectionMode = SelectionMode.MultiExtended };
            list.Items.AddRange(names.Cast<object>().ToArray());
            var select = Theme.Button("添加所选进程", (_, _) => { dialog.DialogResult = DialogResult.OK; }); select.Dock = DockStyle.Bottom;
            dialog.Controls.Add(list); dialog.Controls.Add(select);
            if (dialog.ShowDialog(this) == DialogResult.OK)
                processes.Text = string.Join(Environment.NewLine, processes.Lines.Concat(list.SelectedItems.Cast<string>()).Where(n => !string.IsNullOrWhiteSpace(n)).Distinct(StringComparer.OrdinalIgnoreCase));
        })));
        var interval = new NumericUpDown { Minimum = 2, Maximum = 30, Value = settings.PollSeconds, BackColor = Theme.Raised, ForeColor = Theme.Text, Font = Theme.Font(), Width = 100, AccessibleName = "进程检测间隔秒数" };
        AddRow(stack, Theme.Label("检测间隔（秒，默认 3；手动模式完全停止检测）", 9, Theme.Muted)); AddRow(stack, interval);
        AddRow(stack, Theme.Button("保存设置", (_, _) => Run(() =>
        {
            var next = settings with
            {
                StartWithWindows = startup.Checked,
                CloseToTray = close.Checked,
                RestoreOnExit = restore.Checked,
                GameProcesses = processes.Lines.Select(p => p.Trim()).Where(p => p.Length > 0).Distinct(StringComparer.OrdinalIgnoreCase).ToList(),
                GameProfileId = (gameProfile.SelectedItem as FilterProfile)?.Id,
                PollSeconds = (int)interval.Value
            };
            next.Validate();
            var previous = StartupRegistration.IsEnabled();
            StartupRegistration.SetEnabled(next.StartWithWindows);
            try { SaveSettings(next); }
            catch { StartupRegistration.SetEnabled(previous); throw; }
            ConfigureWatcher();
            SetInfo("设置已保存。");
        }), true));
        AddRow(stack, Theme.Label("方案管理", 13, Theme.Accent, FontStyle.Bold));
        var management = new FlowLayoutPanel { AutoSize = true, Dock = DockStyle.Top };
        management.Controls.Add(Theme.Button("导出当前方案", (_, _) => Run(ExportProfile)));
        management.Controls.Add(Theme.Button("导入方案", (_, _) => Run(() => { ImportProfile(); PopulateGameProfiles(); })));
        management.Controls.Add(Theme.Button("删除当前方案", (_, _) => Run(() => { DeleteProfile(); PopulateGameProfiles(); })));
        AddRow(stack, management);
        AddRow(stack, Theme.Label("使用提示", 13, Theme.Text, FontStyle.Bold));
        var hint = Theme.Label("• 首次请开启游戏后选择真实进程名；内置名称只是候选。\n• 全屏、切换分辨率或其他校色软件可能覆盖曲线，可再次点击应用。\n• 开启全屏滤镜后，图片用于相对对照，不用于精确测色。\n• 程序不会读取游戏内存或注入游戏进程。\n• 强制结束程序无法立即恢复；再次启动会尝试恢复保留的备份。", 9, Theme.Muted);
        hint.MaximumSize = new(760, 0); AddRow(stack, hint);
        AddRow(stack, Theme.Button("打开本机配置目录", (_, _) => Run(() => Process.Start(new ProcessStartInfo(Path.GetDirectoryName(configPath)!) { UseShellExecute = true }))));
    }

    private static Control Spacer(int height) => new Panel { Height = height, Width = 1 };
    private static void AddRow(TableLayoutPanel stack, Control control)
    {
        var row = stack.RowCount++;
        stack.RowStyles.Add(new(SizeType.AutoSize));
        stack.Controls.Add(control, 0, row);
    }
    private void Parameter(TableLayoutPanel stack, string key, string name, decimal min, decimal max, decimal step, string hint = "")
    {
        var parameter = new ParameterControl(name, min, max, step, hint);
        parameter.ValueChanged += (_, _) => DraftChanged(); parameters.Add(key, parameter); AddRow(stack, parameter);
    }

    private void DraftChanged()
    {
        if (loading) return;
        draft = draft with
        {
            Color = new()
            {
                Brightness = (int)parameters["brightness"].Value,
                Contrast = (int)parameters["contrast"].Value,
                Gamma = parameters["gamma"].Value,
                Shadows = (int)parameters["shadows"].Value,
                Highlights = (int)parameters["highlights"].Value,
                RedGain = (int)parameters["red"].Value,
                GreenGain = (int)parameters["green"].Value,
                BlueGain = (int)parameters["blue"].Value,
                DigitalVibrance = vibranceEnabled.Checked ? (int)parameters["vibrance"].Value : null
            }
        };
        SchedulePreview();
        SetInfo("参数已更新到预览。点击应用可调整显示器，点击保存可保留方案。");
    }

    private void LoadDraft(FilterProfile profile)
    {
        loading = true; draft = profile;
        var color = profile.Color;
        parameters["brightness"].Value = color.Brightness; parameters["contrast"].Value = color.Contrast;
        parameters["gamma"].Value = color.Gamma; parameters["shadows"].Value = color.Shadows; parameters["highlights"].Value = color.Highlights;
        parameters["red"].Value = color.RedGain; parameters["green"].Value = color.GreenGain; parameters["blue"].Value = color.BlueGain;
        parameters["vibrance"].Value = color.DigitalVibrance ?? 50;
        vibranceEnabled.Checked = color.DigitalVibrance is not null; parameters["vibrance"].Enabled = vibranceEnabled.Checked;
        RefreshIcc(); loading = false; Run(UpdatePreview);
    }

    private void UpdatePreview()
    {
        if (suspendedPreview) return;
        var ramp = session.Preview(draft);
        preview.SetRamp(ramp); curve.Ramp = ramp; curve.Invalidate();
    }
    private void SchedulePreview() { if (!Visible || suspendedPreview) return; previewTimer.Stop(); previewTimer.Start(); }

    private void RefreshProfiles()
    {
        var previous = loading; loading = true;
        profiles.DataSource = settings.Profiles.ToList();
        profiles.SelectedItem = settings.Profiles.FirstOrDefault(p => p.Id == draft.Id) ?? settings.SelectedProfile;
        loading = previous;
    }

    private void SelectProfile(FilterProfile profile)
    {
        if (!ConfirmDiscard()) { RefreshProfiles(); return; }
        SaveSettings(settings with { SelectedProfileId = profile.Id }); LoadDraft(profile);
    }

    private void RefreshDisplays()
    {
        loading = true;
        targets = WindowsDisplayBackend.Monitors();
        monitors.DataSource = targets; monitors.SelectedItem = targets.FirstOrDefault(m => m.MonitorId == settings.MonitorId) ?? (settings.MonitorId is null ? targets.FirstOrDefault() : null);
        loading = false;
        if (monitors.SelectedItem is MonitorTarget monitor)
            driver.Text = smoke ? "SDR 工作台 · 硬件状态需本机验证" : WindowsDisplayBackend.NvidiaStatus(monitor);
        else driver.Text = "目标显示器未连接；请选择显示器。";
    }

    private void SelectMonitor()
    {
        if (monitors.SelectedItem is not MonitorTarget monitor) return;
        if (session.Snapshot is not null && session.Snapshot.Monitor.MonitorId != monitor.MonitorId)
        {
            loading = true; monitors.SelectedItem = targets.FirstOrDefault(m => m.MonitorId == session.Snapshot.Monitor.MonitorId); loading = false;
            throw new InvalidOperationException("请先恢复原始色彩，再切换显示器。");
        }
        SaveSettings(settings with { MonitorId = monitor.MonitorId }); driver.Text = WindowsDisplayBackend.NvidiaStatus(monitor);
    }

    private void RefreshIcc()
    {
        var previous = loading; loading = true;
        var paths = backend.ListProfiles();
        if (draft.IccPath is not null && !paths.Contains(draft.IccPath, StringComparer.OrdinalIgnoreCase)) paths.Add(draft.IccPath);
        var choices = new List<IccChoice> { new("不切换 ICC · 使用原始校准", null) };
        choices.AddRange(paths.Select(p => new IccChoice(Path.GetFileName(p), p)));
        icc.DataSource = choices; icc.SelectedItem = choices.FirstOrDefault(p => p.Path == draft.IccPath) ?? choices[0];
        loading = previous;
    }

    private void SaveDraft()
    {
        draft.Validate();
        SaveSettings(settings with { Profiles = settings.Profiles.Select(p => p.Id == draft.Id ? draft : p).ToList(), SelectedProfileId = draft.Id });
        RefreshProfiles(); SetInfo("方案已保存。");
    }

    private void DuplicateProfile()
    {
        var name = AskName(draft.Name + " · 副本"); if (name is null) return;
        var profile = draft with { Id = Guid.NewGuid().ToString("N"), Name = name };
        SaveSettings(settings with { Profiles = [.. settings.Profiles, profile], SelectedProfileId = profile.Id });
        draft = profile; RefreshProfiles(); LoadDraft(profile); SetInfo("新方案已保存。");
    }

    private string? AskName(string initial)
    {
        using var dialog = new Form { Text = "方案名称", ClientSize = new(440, 142), FormBorderStyle = FormBorderStyle.FixedDialog, StartPosition = FormStartPosition.CenterParent, MinimizeBox = false, MaximizeBox = false, BackColor = Theme.Surface, ForeColor = Theme.Text, Padding = new(20) };
        var name = Theme.TextBox(initial); name.MaxLength = 80;
        var save = Theme.Button("保存", (_, _) => { if (!string.IsNullOrWhiteSpace(name.Text)) dialog.DialogResult = DialogResult.OK; }, true); save.Dock = DockStyle.Bottom;
        dialog.Controls.Add(name); dialog.Controls.Add(save); dialog.AcceptButton = save;
        return dialog.ShowDialog(this) == DialogResult.OK ? name.Text.Trim() : null;
    }

    private void DeleteProfile()
    {
        if (settings.Profiles.Count == 1) throw new InvalidOperationException("至少需要保留一个方案。");
        if (MessageBox.Show(this, $"删除方案“{draft.Name}”？", "删除方案", MessageBoxButtons.YesNo, MessageBoxIcon.Question) != DialogResult.Yes) return;
        var list = settings.Profiles.Where(p => p.Id != draft.Id).ToList();
        SaveSettings(settings with { Profiles = list, SelectedProfileId = list[0].Id, GameProfileId = settings.GameProfileId == draft.Id ? list[0].Id : settings.GameProfileId });
        draft = list[0]; RefreshProfiles(); LoadDraft(draft);
    }

    private void ExportProfile()
    {
        using var dialog = new SaveFileDialog { Filter = "滤镜方案 (*.json)|*.json", FileName = "filter-profile.json" };
        if (dialog.ShowDialog(this) != DialogResult.OK) return;
        draft.Validate(); Storage.Write(dialog.FileName, draft); SetInfo("方案已导出。ICC 文件需单独导出并在另一台电脑重新选择。");
    }

    private void ImportProfile()
    {
        using var dialog = new OpenFileDialog { Filter = "滤镜方案 (*.json)|*.json" };
        if (dialog.ShowDialog(this) != DialogResult.OK) return;
        var imported = Storage.Read<FilterProfile>(dialog.FileName); imported.Validate();
        imported = imported with { Id = Guid.NewGuid().ToString("N"), IccPath = null };
        SaveSettings(settings with { Profiles = [.. settings.Profiles, imported], SelectedProfileId = imported.Id });
        draft = imported; RefreshProfiles(); LoadDraft(imported); SetInfo("方案已导入。请在本机重新选择 ICC，避免应用外部路径。");
    }

    private void ImportImage()
    {
        using var dialog = new OpenFileDialog { Filter = "参考图片 (*.png;*.jpg;*.jpeg;*.bmp)|*.png;*.jpg;*.jpeg;*.bmp", Title = "导入参考图片（仅在本机读取）" };
        if (dialog.ShowDialog(this) == DialogResult.OK) LoadImage(dialog.FileName);
    }
    private void LoadImage(string path)
    {
        preview.LoadImage(path); imagePath = path; imageName.Text = Path.GetFileName(path); toolTip.SetToolTip(imageName, path); UpdatePreview();
    }
    private void ImportIcc()
    {
        using var dialog = new OpenFileDialog { Filter = "ICC 配置 (*.icc;*.icm)|*.icc;*.icm" };
        if (dialog.ShowDialog(this) != DialogResult.OK) return;
        var path = backend.ImportProfile(dialog.FileName); var data = IccProfile.Read(path);
        draft = draft with { IccPath = path }; RefreshIcc(); UpdatePreview(); SetInfo($"ICC 已导入：{data.Description}。");
    }
    private void ExportIcc()
    {
        if (draft.IccPath is null) throw new InvalidOperationException("请先选择 ICC。");
        using var dialog = new SaveFileDialog { Filter = "ICC 配置 (*.icc)|*.icc|ICM 配置 (*.icm)|*.icm", FileName = Path.GetFileName(draft.IccPath) };
        if (dialog.ShowDialog(this) != DialogResult.OK) return;
        if (Path.GetFullPath(dialog.FileName).Equals(Path.GetFullPath(draft.IccPath), StringComparison.OrdinalIgnoreCase)) throw new IOException("导出位置与源文件相同。");
        File.Copy(draft.IccPath, dialog.FileName, overwrite: true); SetInfo("ICC 已导出。");
    }

    private void ApplyDraft() => Run(() => { Apply(draft); watcher.SuppressCurrentGame(); });
    private void Apply(FilterProfile profile)
    {
        if (recoveryPending) throw new InvalidOperationException("上次备份尚未恢复，请先点击恢复原始色彩。");
        if (monitors.SelectedItem is not MonitorTarget monitor) throw new InvalidOperationException("请先选择已连接的显示器。");
        session.Apply(monitor, profile); UpdateStatus(); UpdatePreview();
        SetInfo($"已应用“{profile.Name}”。图片用于相对对照；全屏或分辨率变化后可重新应用。");
    }
    private void Restore(bool manual) => Run(() =>
    {
        session.Restore(); recoveryPending = false; autoOwned = false;
        if (manual) watcher.SuppressCurrentGame();
        UpdateStatus(); UpdatePreview(); SetInfo("已恢复启用前的原始色彩。");
    });
    private void CompareOriginal() => Run(() =>
    {
        if (session.IsComparing && session.ActiveProfile is { } active) Apply(active);
        else if (session.IsActive) { session.CompareOriginal(); UpdateStatus(); SetInfo("正在查看原始色彩。再次点击 A/B 返回已应用方案。"); }
        else SetInfo("请先开启滤镜，再使用显示器 A/B 对照。");
    });
    private void ToggleFilter() { if (session.IsActive || session.IsComparing) Restore(manual: true); else ApplyDraft(); }

    private void ConfigureWatcher()
    {
        watcherGeneration++;
        processTimer.Stop(); watcher.Reset(); processTimer.Interval = settings.PollSeconds * 1000;
        if (settings.FollowGame && !smoke) processTimer.Start();
    }
    private async Task CheckGame()
    {
        if (busy || !settings.FollowGame || recoveryPending) return;
        busy = true;
        var generation = watcherGeneration;
        var names = settings.GameProcesses.ToList();
        try
        {
            var present = await Task.Run(() => names.Any(name =>
            {
                var processes = Process.GetProcessesByName(Path.GetFileNameWithoutExtension(name));
                try { return processes.Length > 0; } finally { foreach (var process in processes) process.Dispose(); }
            }));
            if (IsDisposed || quitting || !settings.FollowGame || generation != watcherGeneration) return;
            var transition = watcher.Observe(present);
            if (transition == GameTransition.Started)
            {
                Apply(settings.GameProfile); autoOwned = true;
            }
            else if (transition == GameTransition.Exited && autoOwned)
                Restore(manual: false);
        }
        catch (Exception error) { SetError($"游戏联动失败：{Program.ErrorText(error)}"); }
        finally { busy = false; }
    }

    private void SaveSettings(AppSettings next) { next.Validate(); Storage.Write(configPath, next); settings = next; }
    private bool ConfirmDiscard() => settings.Profiles.FirstOrDefault(p => p.Id == draft.Id) == draft ||
        MessageBox.Show(this, "当前方案有未保存的调节。切换后放弃这些调节？", "未保存的调节", MessageBoxButtons.YesNo, MessageBoxIcon.Question) == DialogResult.Yes;
    private void ShowPage(bool showSettings)
    {
        if (showSettings) refreshGameProfiles?.Invoke();
        settingsPage.Visible = showSettings; tuningPage.Visible = !showSettings;
    }

    private void UpdateStatus()
    {
        var text = session.IsComparing ? "● 原始对照中" : session.IsActive ? "● 滤镜已开启" : session.Snapshot is not null ? "● 等待恢复" : "○ 滤镜未开启";
        status.Text = text; status.ForeColor = session.IsActive ? Theme.Accent : session.Snapshot is not null ? Theme.Warning : Theme.Muted;
        if (tray is not null) tray.Text = $"DeltaForce Filter · {text}";
    }
    internal void SetError(string message)
    {
        statusDetail.Text = message; statusDetail.ForeColor = Theme.Error; toolTip.SetToolTip(statusDetail, message); UpdateStatus();
        if (!Visible && !smoke && tray is not null) tray.ShowBalloonTip(6000, "DeltaForce Filter", message[..Math.Min(message.Length, 250)], ToolTipIcon.Error);
    }
    private void SetInfo(string message) { statusDetail.Text = message; statusDetail.ForeColor = Theme.Muted; toolTip.SetToolTip(statusDetail, message); }
    private void Run(Action action)
    {
        try { action(); }
        catch (Exception error) { SetError(Program.ErrorText(error)); }
    }
    private void HideToTray()
    {
        previewTimer.Stop(); Hide(); suspendedPreview = true;
        // Release large image buffers while the user is playing.
        preview.LoadTinyPlaceholder();
    }
    private void OpenWindow()
    {
        Show(); WindowState = FormWindowState.Normal; Activate();
        if (suspendedPreview)
        {
            suspendedPreview = false;
            Run(() => { if (imagePath is null) preview.ResetImage(); else preview.LoadImage(imagePath); UpdatePreview(); });
        }
    }
    private void Quit()
    {
        if (!ConfirmDiscard()) return;
        if (settings.RestoreOnExit && session.Snapshot is not null)
        {
            try { session.Restore(); }
            catch (Exception error) { OpenWindow(); SetError(Program.ErrorText(error)); return; }
        }
        quitting = true; Close();
    }
    private void HandleFormClosing(object? sender, FormClosingEventArgs e)
    {
        if (smoke || quitting) return;
        if (e.CloseReason == CloseReason.UserClosing && settings.CloseToTray)
        {
            e.Cancel = true; HideToTray(); return;
        }
        if (e.CloseReason == CloseReason.UserClosing && !ConfirmDiscard()) { e.Cancel = true; return; }
        if (settings.RestoreOnExit && session.Snapshot is not null)
        {
            try { session.Restore(); }
            catch (Exception error)
            {
                SetError(Program.ErrorText(error));
                if (e.CloseReason == CloseReason.UserClosing) { e.Cancel = true; return; }
            }
        }
        quitting = true;
    }
    internal void CloseForSmokeTest() { quitting = true; Close(); }

    internal void CaptureSmokeTest(string output)
    {
        if (Marshal.SizeOf<NativeMethods.DisplayDevice>() != 840 ||
            Marshal.SizeOf<NativeMethods.DisplayPath>() != 72 ||
            Marshal.SizeOf<NativeMethods.SourceName>() != 84 ||
            Marshal.SizeOf<NativeMethods.AdvancedColor>() != 32)
            throw new InvalidOperationException("Win32 显示 API 结构布局无效。");
        preview.VerifyImagePipeline();
        UpdatePreview();
        // DrawToBitmap does not need the entire window to fit the CI virtual desktop.
        ClientSize = new(1360, 900);
        CaptureWindowImage(output);
        ShowPage(true);
        CaptureWindowImage(Path.ChangeExtension(output, "settings.png"));
        ShowPage(false);
        ClientSize = new(1140, 780);
        CaptureWindowImage(Path.ChangeExtension(output, "compact.png"));
        using var process = Process.GetCurrentProcess();
        process.Refresh();
        var visibleMemory = process.WorkingSet64;
        HideToTray();
        // Exclude startup layout, serialization and tiered JIT work from the idle sample.
        for (var index = 0; index < 50; index++) { Application.DoEvents(); Thread.Sleep(100); }
        var cpu = process.TotalProcessorTime;
        var stopwatch = Stopwatch.StartNew();
        for (var index = 0; index < 50; index++) { Application.DoEvents(); Thread.Sleep(100); }
        process.Refresh();
        Storage.Write(Path.ChangeExtension(output, "report.json"), new
        {
            ImageImportAndGammaPreview = "passed",
            Win32StructureLayout = "passed",
            DeviceDpi,
            VisibleWorkingSetMiB = visibleMemory / 1048576.0,
            TrayWorkingSetMiB = process.WorkingSet64 / 1048576.0,
            IdleWarmupSeconds = 5,
            IdleSampleSeconds = stopwatch.Elapsed.TotalSeconds,
            IdleCpuSeconds = (process.TotalProcessorTime - cpu).TotalSeconds,
            ProcessPollingEnabled = processTimer.Enabled,
            Environment = "GitHub Actions Windows runner, manual mode; not a game PC benchmark"
        });
    }

    private void CaptureWindowImage(string output)
    {
        PerformLayout(); Invalidate(invalidateChildren: true); Refresh(); Application.DoEvents();
        var content = Controls[0];
        using var bitmap = new Bitmap(content.Width, content.Height);
        content.DrawToBitmap(bitmap, new(0, 0, bitmap.Width, bitmap.Height));
        bitmap.Save(output);
    }
    private void DisposeResources()
    {
        processTimer.Stop(); processTimer.Dispose(); previewTimer.Dispose();
        if (!smoke) { NativeMethods.UnregisterHotKey(Handle, 1); NativeMethods.UnregisterHotKey(Handle, 2); }
        tray.Visible = false; tray.Dispose(); toolTip.Dispose();
    }
    protected override void WndProc(ref Message message)
    {
        const int hotkey = 0x0312;
        const int displayChange = 0x007E;
        if (message.Msg == hotkey) { if (message.WParam == 1) ToggleFilter(); else if (message.WParam == 2) Restore(manual: true); }
        if (message.Msg == displayChange && !loading && IsHandleCreated)
            BeginInvoke(() => Run(() => { RefreshDisplays(); SetInfo("显示配置已变化。如滤镜被系统重置，请重新应用。断开的显示器备份仍保留。"); }));
        base.WndProc(ref message);
    }

    protected override void OnHandleCreated(EventArgs e)
    {
        base.OnHandleCreated(e);
        var dark = 1;
        // Older Windows versions may not expose the cosmetic dark titlebar attribute.
        NativeMethods.DwmSetWindowAttribute(Handle, 20, ref dark, sizeof(int));
    }

    private sealed record IccChoice(string Name, string? Path);
}
