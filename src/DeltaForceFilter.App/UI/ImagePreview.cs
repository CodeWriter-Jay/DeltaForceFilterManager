using System.ComponentModel;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using DeltaForceFilter.Core;

namespace DeltaForceFilter.App.UI;

internal sealed class ImagePreview : Control
{
    private Bitmap original;
    private Bitmap adjusted;
    private double split = 0.5;
    private bool comparing = true;

    internal ImagePreview()
    {
        SetStyle(ControlStyles.UserPaint | ControlStyles.AllPaintingInWmPaint | ControlStyles.OptimizedDoubleBuffer | ControlStyles.Selectable, true);
        BackColor = Theme.Background;
        original = CalibrationImage();
        adjusted = (Bitmap)original.Clone();
        TabStop = true;
        AccessibleName = "参考图片对照，左右方向键调整分隔线";
        Cursor = Cursors.SizeWE;
    }

    [DesignerSerializationVisibility(DesignerSerializationVisibility.Hidden)]
    internal bool Comparing { get => comparing; set { comparing = value; Invalidate(); } }

    internal void LoadImage(string path)
    {
        if (!new[] { ".png", ".jpg", ".jpeg", ".bmp" }.Contains(Path.GetExtension(path), StringComparer.OrdinalIgnoreCase))
            throw new InvalidDataException("请选择 PNG、JPEG 或 BMP 参考图片。");
        if (new FileInfo(path).Length > 25 * 1024 * 1024) throw new InvalidDataException("参考图片最大为 25 MB。");
        using var source = Image.FromFile(path, useEmbeddedColorManagement: false);
        if ((long)source.Width * source.Height > 32_000_000) throw new InvalidDataException("图片最多 3200 万像素。");
        var scale = Math.Min(1, Math.Min(1600.0 / source.Width, 900.0 / source.Height));
        var image = new Bitmap(Math.Max(1, (int)(source.Width * scale)), Math.Max(1, (int)(source.Height * scale)), PixelFormat.Format32bppArgb);
        try
        {
            using (var graphics = Graphics.FromImage(image))
            {
                graphics.Clear(Color.Black);
                graphics.InterpolationMode = InterpolationMode.HighQualityBicubic;
                graphics.DrawImage(source, new Rectangle(0, 0, image.Width, image.Height));
            }
            var copy = (Bitmap)image.Clone();
            original.Dispose(); adjusted.Dispose();
            original = image; adjusted = copy;
        }
        catch { image.Dispose(); throw; }
        Invalidate();
    }

    internal void ResetImage()
    {
        var image = CalibrationImage();
        original.Dispose(); adjusted.Dispose();
        original = image; adjusted = (Bitmap)image.Clone();
        Invalidate();
    }

    internal void LoadTinyPlaceholder()
    {
        original.Dispose(); adjusted.Dispose();
        original = new Bitmap(1, 1, PixelFormat.Format32bppArgb);
        adjusted = new Bitmap(1, 1, PixelFormat.Format32bppArgb);
    }

    internal void VerifyImagePipeline()
    {
        var path = Path.Combine(Path.GetTempPath(), $"filter-image-test-{Guid.NewGuid():N}.png");
        try
        {
            using (var image = new Bitmap(32, 32, PixelFormat.Format32bppArgb))
            {
                using var graphics = Graphics.FromImage(image);
                graphics.Clear(Color.FromArgb(64, 128, 192));
                image.Save(path, ImageFormat.Png);
            }
            LoadImage(path);
            // File deletion verifies that importing does not leave the source locked.
            File.Delete(path);
            SetRamp(ToneCurve.Identity());
            if (adjusted.GetPixel(16, 16).ToArgb() != Color.FromArgb(64, 128, 192).ToArgb())
                throw new InvalidOperationException("参考图片导入或中性预览验证失败。");
            SetRamp(ToneCurve.Build(new() { Gamma = 1.2 }));
            var pixel = adjusted.GetPixel(16, 16);
            if (pixel.R <= 64 || pixel.G <= 128 || pixel.B <= 192)
                throw new InvalidOperationException("参考图片 Gamma 预览验证失败。");
        }
        finally { if (File.Exists(path)) File.Delete(path); }
        ResetImage();
    }

    internal unsafe void SetRamp(ushort[] ramp)
    {
        if (ramp.Length != 768) throw new ArgumentException("曲线长度无效。", nameof(ramp));
        var rectangle = new Rectangle(0, 0, original.Width, original.Height);
        var source = original.LockBits(rectangle, ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
        try
        {
            var target = adjusted.LockBits(rectangle, ImageLockMode.WriteOnly, PixelFormat.Format32bppArgb);
            try
            {
                for (var row = 0; row < original.Height; row++)
                {
                    var input = (byte*)source.Scan0 + row * source.Stride;
                    var output = (byte*)target.Scan0 + row * target.Stride;
                    for (var column = 0; column < original.Width; column++)
                    {
                        var pixel = column * 4;
                        output[pixel] = (byte)((ramp[512 + input[pixel]] + 128) / 257);
                        output[pixel + 1] = (byte)((ramp[256 + input[pixel + 1]] + 128) / 257);
                        output[pixel + 2] = (byte)((ramp[input[pixel + 2]] + 128) / 257);
                        output[pixel + 3] = 255;
                    }
                }
            }
            finally { adjusted.UnlockBits(target); }
        }
        finally { original.UnlockBits(source); }
        Invalidate();
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        base.OnPaint(e);
        e.Graphics.InterpolationMode = InterpolationMode.HighQualityBicubic;
        var scale = Math.Min(Width / (float)original.Width, Height / (float)original.Height);
        var width = Math.Max(1, (int)(original.Width * scale));
        var height = Math.Max(1, (int)(original.Height * scale));
        var rectangle = new Rectangle((Width - width) / 2, (Height - height) / 2, width, height);
        e.Graphics.DrawImage(adjusted, rectangle);
        if (Comparing)
        {
            var position = rectangle.Left + (int)(rectangle.Width * split);
            var state = e.Graphics.Save();
            e.Graphics.SetClip(new Rectangle(rectangle.Left, rectangle.Top, position - rectangle.Left, rectangle.Height));
            e.Graphics.DrawImage(original, rectangle);
            e.Graphics.Restore(state);
            using var pen = new Pen(Theme.Accent, 2);
            e.Graphics.DrawLine(pen, position, rectangle.Top, position, rectangle.Bottom);
            using var brush = new SolidBrush(Theme.Accent);
            e.Graphics.FillEllipse(brush, position - 15, Height / 2 - 15, 30, 30);
            using var arrowFont = Theme.Font(11);
            TextRenderer.DrawText(e.Graphics, "↔", arrowFont, new Rectangle(position - 15, Height / 2 - 15, 30, 30), Theme.Background, TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
            Badge(e.Graphics, "原图", rectangle.Left + 12, rectangle.Top + 12);
            Badge(e.Graphics, "调整后", rectangle.Right - 84, rectangle.Top + 12);
        }
        if (Focused) ControlPaint.DrawFocusRectangle(e.Graphics, new(2, 2, Width - 4, Height - 4), Theme.Text, BackColor);
    }

    private static void Badge(Graphics graphics, string text, int x, int y)
    {
        using var brush = new SolidBrush(Color.FromArgb(220, 16, 21, 28));
        graphics.FillRectangle(brush, x, y, 70, 28);
        using var font = Theme.Font(9);
        TextRenderer.DrawText(graphics, text, font, new Rectangle(x, y, 70, 28), Theme.Text, TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
    }

    protected override void OnMouseDown(MouseEventArgs e) { base.OnMouseDown(e); if (e.Button == MouseButtons.Left) { Focus(); Capture = true; MoveSplit(e.X); } }
    protected override void OnMouseMove(MouseEventArgs e) { base.OnMouseMove(e); if (Capture) MoveSplit(e.X); }
    protected override void OnMouseUp(MouseEventArgs e) { base.OnMouseUp(e); Capture = false; }
    protected override bool IsInputKey(Keys keyData) => keyData is Keys.Left or Keys.Right || base.IsInputKey(keyData);
    protected override void OnKeyDown(KeyEventArgs e)
    {
        base.OnKeyDown(e);
        if (e.KeyCode is not (Keys.Left or Keys.Right)) return;
        split = Math.Clamp(split + (e.KeyCode == Keys.Left ? -0.02 : 0.02), 0, 1);
        Invalidate(); e.Handled = true;
    }
    private void MoveSplit(int x)
    {
        var width = Math.Min(Width, Height * original.Width / (double)original.Height);
        split = Math.Clamp((x - (Width - width) / 2) / Math.Max(1, width), 0, 1);
        Invalidate();
    }

    private static Bitmap CalibrationImage()
    {
        var image = new Bitmap(1200, 700, PixelFormat.Format32bppArgb);
        using var g = Graphics.FromImage(image);
        g.Clear(Color.FromArgb(17, 23, 30));
        using var font = Theme.Font(16, FontStyle.Bold);
        using var small = Theme.Font(11);
        using var text = new SolidBrush(Theme.Text);
        using var muted = new SolidBrush(Theme.Muted);
        g.DrawString("看见暗部，保留高光。", font, text, 54, 60);
        g.DrawString("拖入游戏截图或导入参考图片，找到适合你的画面", small, muted, 54, 106);
        for (var x = 0; x < 1092; x++)
        {
            var value = (int)Math.Round(x / 1091.0 * 255);
            using var pen = new Pen(Color.FromArgb(value, value, value));
            g.DrawLine(pen, 54 + x, 178, 54 + x, 310);
        }
        g.DrawString("SHADOWS", small, muted, 54, 324);
        g.DrawString("HIGHLIGHTS", small, muted, 1040, 324);
        var colors = new[] { Color.FromArgb(244, 90, 92), Color.FromArgb(244, 180, 92), Color.FromArgb(244, 225, 130), Color.FromArgb(94, 224, 183), Color.FromArgb(99, 169, 234), Color.FromArgb(178, 130, 232) };
        for (var index = 0; index < colors.Length; index++)
        {
            using var brush = new SolidBrush(colors[index]);
            g.FillRectangle(brush, 54 + index * 185, 398, 167, 90);
        }
        for (var index = 0; index < 16; index++)
        {
            var value = index * 2;
            using var brush = new SolidBrush(Color.FromArgb(value, value, value));
            g.FillRectangle(brush, 54 + index * 69, 542, 62, 58);
        }
        g.DrawString("黑阶 0–30  ·  调节暗部时观察相邻色块是否仍可区分", small, muted, 54, 620);
        return image;
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing) { original.Dispose(); adjusted.Dispose(); }
        base.Dispose(disposing);
    }
}

internal sealed class CurveView : Control
{
    [DesignerSerializationVisibility(DesignerSerializationVisibility.Hidden)]
    internal ushort[] Ramp { get; set; } = ToneCurve.Identity();
    internal CurveView() { DoubleBuffered = true; BackColor = Theme.Surface; }
    protected override void OnPaint(PaintEventArgs e)
    {
        base.OnPaint(e);
        e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
        var area = new Rectangle(36, 16, Math.Max(1, Width - 56), Math.Max(1, Height - 42));
        using var grid = new Pen(Theme.Border);
        for (var index = 0; index <= 4; index++)
        {
            var x = area.X + area.Width * index / 4;
            var y = area.Y + area.Height * index / 4;
            e.Graphics.DrawLine(grid, x, area.Top, x, area.Bottom);
            e.Graphics.DrawLine(grid, area.Left, y, area.Right, y);
        }
        using var identity = new Pen(Theme.Muted) { DashStyle = DashStyle.Dash };
        e.Graphics.DrawLine(identity, area.Left, area.Bottom, area.Right, area.Top);
        var colors = new[] { Color.FromArgb(248, 130, 133), Theme.Accent, Color.FromArgb(114, 172, 246) };
        for (var channel = 0; channel < 3; channel++)
        {
            var points = Enumerable.Range(0, 256).Select(index => new PointF(area.Left + index / 255f * area.Width, area.Bottom - Ramp[channel * 256 + index] / 65535f * area.Height)).ToArray();
            using var pen = new Pen(colors[channel], 1.7f);
            e.Graphics.DrawLines(pen, points);
        }
        using var font = Theme.Font(8);
        TextRenderer.DrawText(e.Graphics, "暗部", font, new Point(area.Left, area.Bottom + 6), Theme.Muted);
        TextRenderer.DrawText(e.Graphics, "高光", font, new Point(area.Right - 32, area.Bottom + 6), Theme.Muted);
    }
}
