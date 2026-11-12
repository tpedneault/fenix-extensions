"""matplotlib's backend while Fenix's Jupyter kernel runs cells itself
(MPLBACKEND=module://fenix_mpl): figures drawn by Agg and shown in the
notebook as pictures, on plt.show() and at the end of each cell, as
Jupyter's inline backend shows them."""

import sys

from matplotlib.backend_bases import FigureManagerBase
from matplotlib.backends.backend_agg import FigureCanvasAgg

FigureCanvas = FigureCanvasAgg
FigureManager = FigureManagerBase


def show(*args, **kwargs):
    hook = getattr(sys.modules.get("__main__"), "fenix_show", None)
    if hook is not None:
        hook()
