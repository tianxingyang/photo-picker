"""Analysis thresholds — global hard-coded for MVP (design §6).

Initial values are a starting point pending fixture calibration; payload may
override them (parameter slot reserved). Tests assert mostly relative orderings
and only "obvious" samples against absolute thresholds to avoid brittle tests.
"""

from __future__ import annotations

# blur/exposure normalize the longest side to this before computing, so
# blur_score is comparable across source resolutions.
NORM_MAX_SIDE = 1024

# Laplacian variance below this => blurry.
BLUR_VAR_THRESHOLD = 100.0

# Normalized grayscale mean (in [0, 1]) above/below => over/under exposed.
OVER_MEAN = 0.80
UNDER_MEAN = 0.20

# Fraction of pixels clipped at the bright/dark extreme that flags over/under.
CLIP_RATIO = 0.50
