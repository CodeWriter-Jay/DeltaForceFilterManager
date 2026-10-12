using System.Drawing.Drawing2D;

namespace DeltaForceFilter.App.UI;

internal static class Theme
{
    internal static readonly Color Background = Color.FromArgb(16, 21, 28);
    internal static readonly Color Surface = Color.FromArgb(24, 31, 40);
    internal static readonly Color Raised = Color.FromArgb(34, 43, 54);
    internal static readonly Color Border = Color.FromArgb(48, 60, 72);
    internal static readonly Color Text = Color.FromArgb(232, 239, 243);
    internal static readonly Color Muted = Color.FromArgb(143, 159, 173);
    internal static readonly Color Accent = Color.FromArgb(94, 224, 183);
    internal static readonly Color Warning = Color.FromArgb(245, 187, 107);
    internal static readonly Color Error = Color.FromArgb(248, 130, 133);

    internal static Font Font(float size = 10, FontStyle style = FontStyle.Regular) => new("Microsoft YaHei UI", size, style, GraphicsUnit.Point);
    internal static Label Label(string text, float size = 10, Color? color = null, FontStyle style = FontStyle.Regular) =>
        new() { Text = text, Font = Font(size, style), ForeColor = color ?? Text, AutoSize = true, Margin = new(0, 4, 0, 8) };

    internal static Button Button(string text, EventHandler action, bool primary = false)
    {
        var button = new Button
        {
            Text = text,
            AutoSize = true,
            MinimumSize = new(88, 36),
            Padding = new(10, 4, 10, 4),
            FlatStyle = FlatStyle.Flat,
            BackColor = primary ? Accent : Raised,
            ForeColor = primary ? Background : Text,
            Font = Font(9.5f, FontStyle.Bold),
            Cursor = Cursors.Hand,
            Margin = new(0, 0, 8, 8),
            UseVisualStyleBackColor = false
        };
        button.FlatAppearance.BorderColor = primary ? Accent : Border;
        button.FlatAppearance.MouseOverBackColor = primary ? Color.FromArgb(133, 237, 204) : Border;
        button.Click += action;
        return button;
    }

    internal static ComboBox Combo()
    {
        var combo = new ComboBox
        {
            DropDownStyle = ComboBoxStyle.DropDownList,
            FlatStyle = FlatStyle.Flat,
            DrawMode = DrawMode.OwnerDrawFixed,
            ItemHeight = 24,
            BackColor = Raised,
            ForeColor = Text,
            Font = Font(9.5f),
            Dock = DockStyle.Top,
            IntegralHeight = false,
            DropDownHeight = 280,
            Margin = new(0, 0, 0, 12)
        };
        combo.DrawItem += (_, e) =>
        {
            using var brush = new SolidBrush((e.State & DrawItemState.Selected) != 0 ? Border : Raised);
            e.Graphics.FillRectangle(brush, e.Bounds);
            var label = e.Index < 0 ? "" : combo.GetItemText(combo.Items[e.Index]);
            var bounds = new Rectangle(e.Bounds.X + 8, e.Bounds.Y, Math.Max(1, e.Bounds.Width - 12), e.Bounds.Height);
            TextRenderer.DrawText(e.Graphics, label, combo.Font, bounds, Text, TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis | TextFormatFlags.NoPrefix);
            e.DrawFocusRectangle();
        };
        return combo;
    }

    internal static CheckBox Check(string text, bool value) => new()
    {
        Text = text,
        Checked = value,
        AutoSize = true,
        Font = Font(10),
        ForeColor = Text,
        Margin = new(0, 10, 0, 10),
        Padding = new(0, 3, 0, 3)
    };

    internal static TextBox TextBox(string text = "") => new()
    {
        Text = text,
        BorderStyle = BorderStyle.FixedSingle,
        BackColor = Raised,
        ForeColor = Text,
        Font = Font(10),
        Dock = DockStyle.Top,
        Margin = new(0, 0, 0, 12)
    };

    internal static GraphicsPath Rounded(RectangleF rectangle, float radius)
    {
        var path = new GraphicsPath();
        var diameter = Math.Min(radius * 2, Math.Min(rectangle.Width, rectangle.Height));
        path.AddArc(rectangle.X, rectangle.Y, diameter, diameter, 180, 90);
        path.AddArc(rectangle.Right - diameter, rectangle.Y, diameter, diameter, 270, 90);
        path.AddArc(rectangle.Right - diameter, rectangle.Bottom - diameter, diameter, diameter, 0, 90);
        path.AddArc(rectangle.X, rectangle.Bottom - diameter, diameter, diameter, 90, 90);
        path.CloseFigure();
        return path;
    }
}

internal sealed class Card : Panel
{
    internal Card() { DoubleBuffered = true; BackColor = Theme.Surface; Padding = new(18); }
    protected override void OnPaint(PaintEventArgs e)
    {
        base.OnPaint(e);
        e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
        using var path = Theme.Rounded(new(0, 0, Width - 1, Height - 1), 12);
        using var pen = new Pen(Theme.Border);
        e.Graphics.DrawPath(pen, path);
    }
}
