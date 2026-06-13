// Subtractive synthesis: broadband noise carved by bandpass filters
// into the same shape as the reference sample. Each bandpass is a
// resonant peak; the noise running through it provides the "jagged
// mountains" texture you saw in the reference's spectrum.
//
// Tuning order: load the reference, then adjust each filter's
// center_hz to land on the corresponding peak you see in the
// reference's spectrum. Q controls peak width — higher = narrower.
// The third filter is intentionally low-Q (1.5) to scoop a broad
// shoulder of high-frequency content rather than a narrow peak.

patch("ping", "one_shot",
    mix([
        // Main resonance — narrow peak around 1 kHz
        noise("white", 3.0).bandpass(1000.0, 200.0).gain(60),
        // Second resonance — octave above, similar width
        noise("white", 3.0).bandpass(2050.0, 200.0).gain(20),
        // Broad high-frequency shoulder — low Q, centred mid-shoulder
        //noise("white", 1.0).bandpass(3200.0, 4).gain(0.15),
    ]).lowpass(6000.0, 0.4).tremolo(4,0.2).env(0.008, 1.1).fade_out(0.1).gain(0.55));
