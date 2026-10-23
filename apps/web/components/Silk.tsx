/**
 * The site's backdrop: folds of peach silk fixed behind every page, so the
 * ground stays the same while the content scrolls over it. Drawn rather than
 * photographed so it follows the palette in both themes.
 *
 * Each drape is its own layer and drifts on its own slow cycle. Only
 * transform and opacity move, so the blurred artwork is rasterized once and
 * never repainted. A soft clearing keeps the reading column on paper, and a
 * fine grain stops the blur from banding.
 */
export function Backdrop() {
  return (
    <div aria-hidden="true" className="backdrop">
      <svg className="backdrop-layer backdrop-left" viewBox="0 0 1600 1000" preserveAspectRatio="xMinYMid slice">
        <defs>
          <filter id="bd-soft" x="-30%" y="-30%" width="160%" height="160%">
            <feGaussianBlur stdDeviation="22" />
          </filter>
          <filter id="bd-softer" x="-30%" y="-30%" width="160%" height="160%">
            <feGaussianBlur stdDeviation="48" />
          </filter>
          <filter id="bd-sheen" x="-30%" y="-30%" width="160%" height="160%">
            <feGaussianBlur stdDeviation="9" />
          </filter>
        </defs>
        {/* Left drape: falls from the upper left and pools at the bottom. */}
        <g filter="url(#bd-softer)">
          <path d="M-200 120 C 120 90, 330 330, 480 560 S 760 980, 900 1160 L 260 1160 C 180 980, 40 760, -200 700 Z" fill="var(--rm-silk-3)" />
        </g>
        <g filter="url(#bd-soft)" opacity="0.95">
          <path d="M-200 330 C 90 300, 280 500, 400 700 S 600 1020, 700 1160 L 380 1160 C 300 980, 140 800, -200 760 Z" fill="var(--rm-silk-1)" />
          <path d="M-200 520 C 40 520, 180 680, 280 860 S 420 1100, 470 1160 L 150 1160 C 110 1020, 0 920, -200 900 Z" fill="var(--rm-silk-2)" />
          <path d="M-200 40 C 160 10, 390 260, 520 460 S 780 860, 960 1160 L 860 1160 C 700 900, 520 640, 360 460 S 60 170, -200 190 Z" fill="var(--rm-silk-1)" opacity="0.7" />
          <path d="M-200 620 C 0 600, 120 720, 200 880 S 300 1100, 330 1160 L 120 1160 C 90 1060, 20 980, -200 960 Z" fill="var(--rm-silk-2)" opacity="0.9" />
          <path d="M-200 180 C 60 160, 230 330, 340 500 S 520 820, 600 1000 L 540 1000 C 450 830, 300 560, 200 440 S 0 250, -200 260 Z" fill="var(--rm-silk-2)" opacity="0.5" />
        </g>
      </svg>

      <svg className="backdrop-layer backdrop-right" viewBox="0 0 1600 1000" preserveAspectRatio="xMaxYMid slice">
        {/* Right drape: sweeps in from the top right and gathers at the bottom right. */}
        <g filter="url(#bd-softer)">
          <path d="M1240 -160 C 1290 80, 1460 220, 1820 250 L 1820 640 C 1480 600, 1200 380, 1040 -160 Z" fill="var(--rm-silk-3)" />
        </g>
        <g filter="url(#bd-soft)" opacity="0.95">
          <path d="M1080 -160 C 1150 110, 1330 270, 1820 320 L 1820 560 C 1420 530, 1170 330, 980 -160 Z" fill="var(--rm-silk-1)" />
          <path d="M1280 -160 C 1330 40, 1500 140, 1820 150 L 1820 280 C 1500 270, 1290 120, 1200 -160 Z" fill="var(--rm-silk-2)" />
          <path d="M1380 -160 C 1420 0, 1560 60, 1820 60 L 1820 120 C 1560 120, 1400 40, 1330 -160 Z" fill="var(--rm-silk-2)" opacity="0.8" />
          <path d="M1100 1160 C 1260 940, 1480 860, 1820 860 L 1820 1160 Z" fill="var(--rm-silk-1)" opacity="0.8" />
          <path d="M1300 1160 C 1420 1030, 1580 990, 1820 990 L 1820 1160 Z" fill="var(--rm-silk-2)" opacity="0.8" />
          <path d="M1000 1160 C 1180 900, 1420 790, 1820 780 L 1820 840 C 1440 850, 1220 960, 1080 1160 Z" fill="var(--rm-silk-2)" opacity="0.5" />
        </g>
      </svg>

      {/* The sheen along each crest, on its own layer so it can shimmer. */}
      <svg className="backdrop-layer backdrop-sheen-left" viewBox="0 0 1600 1000" preserveAspectRatio="xMinYMid slice">
        <g filter="url(#bd-sheen)">
          <path d="M-200 250 C 120 220, 320 430, 440 630 S 660 1000, 780 1160 L 735 1160 C 615 990, 420 700, 300 540 S 40 300, -200 296 Z" fill="var(--rm-silk-sheen)" />
          <path d="M-200 470 C 60 460, 220 630, 330 820 S 500 1100, 560 1160 L 525 1160 C 450 1080, 290 860, 190 720 S -40 515, -200 512 Z" fill="var(--rm-silk-sheen)" opacity="0.8" />
          <path d="M-200 110 C 140 90, 360 330, 480 520 S 700 880, 860 1160 L 835 1160 C 680 900, 470 600, 350 460 S 60 140, -200 138 Z" fill="var(--rm-silk-sheen)" opacity="0.7" />
        </g>
      </svg>
      <svg className="backdrop-layer backdrop-sheen-right" viewBox="0 0 1600 1000" preserveAspectRatio="xMaxYMid slice">
        <g filter="url(#bd-sheen)">
          <path d="M1160 -160 C 1230 100, 1400 230, 1820 250 L 1820 290 C 1390 280, 1200 140, 1110 -160 Z" fill="var(--rm-silk-sheen)" />
          <path d="M1300 -160 C 1350 30, 1500 110, 1820 112 L 1820 135 C 1500 135, 1330 50, 1265 -160 Z" fill="var(--rm-silk-sheen)" opacity="0.75" />
          <path d="M1180 1160 C 1320 980, 1500 920, 1820 920 L 1820 945 C 1520 950, 1340 1010, 1230 1160 Z" fill="var(--rm-silk-sheen)" />
        </g>
      </svg>

      <div className="backdrop-clear" />
      <svg className="backdrop-grain" width="100%" height="100%">
        <filter id="bd-grain">
          <feTurbulence type="fractalNoise" baseFrequency="0.9" numOctaves="2" stitchTiles="stitch" />
          <feColorMatrix values="0 0 0 0 0.35  0 0 0 0 0.22  0 0 0 0 0.14  0 0 0 0.55 0" />
        </filter>
        <rect width="100%" height="100%" filter="url(#bd-grain)" />
      </svg>
    </div>
  );
}
