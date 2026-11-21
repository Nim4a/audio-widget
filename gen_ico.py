from PIL import Image, ImageDraw
import os
def speaker(sz):
    im = Image.new("RGBA", (sz, sz), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    c = (10, 10, 10, 255)
    u = sz / 32.0
    # body
    d.rectangle([6*u, 12*u, 11*u, 20*u], fill=c)
    d.polygon([(11*u, 12*u), (18*u, 6*u), (18*u, 26*u), (11*u, 20*u)], fill=c)
    # waves
    d.arc([18*u, 10*u, 26*u, 22*u], -60, 60, fill=c, width=max(1, int(2*u)))
    d.arc([20*u, 7*u, 30*u, 25*u], -60, 60, fill=c, width=max(1, int(2*u)))
    return im
outs = [r"Z:\hermes\apps\audio-widget\widget.ico", r"Z:\hermes\projects\audio-widget\widget.ico"]
imgs = [speaker(s) for s in (16, 32, 48, 64)]
for o in outs:
    os.makedirs(os.path.dirname(o), exist_ok=True)
    imgs[3].save(o, format="ICO", sizes=[(16, 16), (32, 32), (48, 48), (64, 64)])
print("ico written", outs[0])
