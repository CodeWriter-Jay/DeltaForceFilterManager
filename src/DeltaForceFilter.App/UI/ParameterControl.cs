using System.ComponentModel;
using System.Drawing.Drawing2D;

namespace DeltaForceFilter.App.UI;

internal sealed class ParameterControl : UserControl
{
    private readonly NumericUpDown number;
    private readonly Slider slider;
    private bool updating;
    internal event EventHandler? ValueChanged;

    internal ParameterControl(string title, decimal minimum, decimal maximum, decimal increment, string description = "")
    {
        Height = description.Length == 0 ? 68 : 85;
        Dock = DockStyle.Top;
        BackColor = Theme.Surface;
        Margin = new(0, 2, 0, 8);
        var layout = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, RowCount = description.Length == 0 ? 2 : 3 };
        layout.ColumnStyles.Add(new(SizeType.Percent, 100));
        layout.ColumnStyles.Add(new(SizeType.Absolute, 76));
        layout.RowStyles.Add(new(SizeType.Absolute, 30));
        layout.RowStyles.Add(new(SizeType.Absolute, 32));
        if (description.Length > 0) layout.RowStyles.Add(new(SizeType.Absolute, 20));
        layout.Controls.Add(Theme.Label(title, 9.5f), 0, 0);
        number = new()
        {
            Minimum = minimum,
            Maximum = maximum,
            Increment = increment,
            DecimalPlaces = increment < 1 ? 2 : 0,
            BackColor = Theme.Raised,
            ForeColor = Theme.Text,
            BorderStyle = BorderStyle.FixedSingle,
            Font = Theme.Font(9),
            Dock = DockStyle.Fill,
            AccessibleName = title,
            Margin = new(0, 0, 0, 4)
        };
        slider = new Slider((double)minimum, (double)maximum, (double)increment) { Dock = DockStyle.Fill, AccessibleName = title };
        layout.Controls.Add(number, 1, 0);
        layout.Controls.Add(slider, 0, 1);
        layout.SetColumnSpan(slider, 2);
        if (description.Length > 0)
        {
            var hint = Theme.Label(description, 8, Theme.Muted);
            hint.Margin = new(0);
            layout.Controls.Add(hint, 0, 2);
            layout.SetColumnSpan(hint, 2);
        }
        Controls.Add(layout);
        number.ValueChanged += (_, _) => Change((double)number.Value);
        slider.ValueChanged += (_, _) => Change(slider.Value);
    }

    [DesignerSerializationVisibility(DesignerSerializationVisibility.Hidden)]
    internal double Value { get => (double)number.Value; set => Change(value, notify: false); }

    private void Change(double value, bool notify = true)
    {
        if (updating) return;
        updating = true;
        number.Value = Math.Clamp((decimal)value, number.Minimum, number.Maximum);
        slider.Value = (double)number.Value;
        updating = false;
        if (notify) ValueChanged?.Invoke(this, EventArgs.Empty);
    }
}

internal sealed class Slider : Control
{
    private readonly double minimum;
    private readonly double maximum;
    private readonly double step;
    private double value;
    internal Slider(double minimum, double maximum, double step)
    {
        this.minimum = minimum; this.maximum = maximum; this.step = step; value = minimum;
    }
    internal event EventHandler? ValueChanged;
    [DesignerSerializationVisibility(DesignerSerializationVisibility.Hidden)]
    internal double Value
    {
        get => value;
        set { this.value = Math.Clamp(value, minimum, maximum); Invalidate(); }
    }

    protected override void OnCreateControl()
    {
        base.OnCreateControl();
        SetStyle(ControlStyles.UserPaint | ControlStyles.AllPaintingInWmPaint | ControlStyles.OptimizedDoubleBuffer | ControlStyles.Selectable, true);
        TabStop = true;
        Cursor = Cursors.Hand;
        AccessibleRole = AccessibleRole.Slider;
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
        var y = Height / 2;
        var width = Math.Max(1, Width - 24);
        var x = 12 + (float)((Value - minimum) / (maximum - minimum)) * width;
        using var background = new Pen(Theme.Border, 4) { StartCap = LineCap.Round, EndCap = LineCap.Round };
        using var active = new Pen(Enabled ? Theme.Accent : Theme.Muted, 4) { StartCap = LineCap.Round, EndCap = LineCap.Round };
        e.Graphics.DrawLine(background, 12, y, Width - 12, y);
        if (x > 12) e.Graphics.DrawLine(active, 12, y, x, y);
        using var brush = new SolidBrush(Enabled ? Theme.Accent : Theme.Muted);
        e.Graphics.FillEllipse(brush, x - 6, y - 6, 12, 12);
        if (Focused) ControlPaint.DrawFocusRectangle(e.Graphics, new(2, 2, Width - 4, Height - 4), Theme.Text, Theme.Surface);
    }

    protected override void OnMouseDown(MouseEventArgs e)
    {
        base.OnMouseDown(e);
        if (e.Button != MouseButtons.Left) return;
        Focus(); Capture = true; SetPosition(e.X);
    }
    protected override void OnMouseMove(MouseEventArgs e) { base.OnMouseMove(e); if (Capture) SetPosition(e.X); }
    protected override void OnMouseUp(MouseEventArgs e) { base.OnMouseUp(e); Capture = false; }
    protected override void OnGotFocus(EventArgs e) { base.OnGotFocus(e); Invalidate(); }
    protected override void OnLostFocus(EventArgs e) { base.OnLostFocus(e); Invalidate(); }
    protected override bool IsInputKey(Keys keyData) => keyData is Keys.Left or Keys.Right or Keys.Up or Keys.Down or Keys.Home or Keys.End || base.IsInputKey(keyData);
    protected override void OnKeyDown(KeyEventArgs e)
    {
        base.OnKeyDown(e);
        var next = e.KeyCode switch
        {
            Keys.Left or Keys.Down => Value - step,
            Keys.Right or Keys.Up => Value + step,
            Keys.Home => minimum,
            Keys.End => maximum,
            _ => Value
        };
        if (next == Value) return;
        Value = next;
        ValueChanged?.Invoke(this, EventArgs.Empty);
        e.Handled = true;
    }

    private void SetPosition(int x)
    {
        var next = minimum + Math.Clamp((x - 12.0) / Math.Max(1, Width - 24), 0, 1) * (maximum - minimum);
        Value = Math.Round((next - minimum) / step) * step + minimum;
        ValueChanged?.Invoke(this, EventArgs.Empty);
    }
}
