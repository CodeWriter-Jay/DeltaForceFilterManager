namespace DeltaForceFilter.Core;

public static class ToneCurve
{
    public static ushort[] Identity() => Enumerable.Range(0, 768).Select(i => (ushort)((i % 256) * 257)).ToArray();

    public static double Map(double input, ColorAdjustment color)
    {
        var contrast = Math.Pow(2, color.Contrast / 82.0);
        var value = Math.Clamp((input - 0.5) * contrast + 0.5 + color.Brightness / 125.0 * 0.15, 0, 1);
        value = Math.Pow(value, 1 / color.Gamma);
        // A smooth shadow lift preserves black and avoids lifting every highlight.
        value += color.Shadows / 100.0 * 4 * value * Math.Pow(1 - value, 3);
        var shoulder = Math.Clamp((value - 0.55) / 0.45, 0, 1);
        shoulder = shoulder * shoulder * (3 - 2 * shoulder);
        value *= 1 - color.Highlights / 100.0 * 0.35 * shoulder;
        return Math.Clamp(value, 0, 1);
    }

    public static ushort[] Build(ColorAdjustment color, ushort[]? calibration = null)
    {
        color.Validate();
        calibration ??= Identity();
        if (calibration.Length != 768) throw new ArgumentException("校准曲线必须包含 3 × 256 个值。", nameof(calibration));
        var gains = new[] { color.RedGain, color.GreenGain, color.BlueGain };
        var ramp = new ushort[768];
        for (var channel = 0; channel < 3; channel++)
        {
            var gain = 1 + gains[channel] / 100.0 * 0.48;
            for (var index = 0; index < 256; index++)
            {
                var position = Map(index / 255.0, color) * 255;
                var lower = (int)position;
                var upper = Math.Min(lower + 1, 255);
                var fraction = position - lower;
                var calibrated = calibration[channel * 256 + lower] * (1 - fraction) + calibration[channel * 256 + upper] * fraction;
                ramp[channel * 256 + index] = (ushort)Math.Clamp(Math.Round(calibrated * gain), 0, ushort.MaxValue);
            }
        }
        return ramp;
    }
}
