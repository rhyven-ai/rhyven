"""Render real captured terminal cells. Optional QA dependency: Pillow."""
import json
from pathlib import Path
import sys
from PIL import Image, ImageDraw, ImageFont

source, destination = sys.argv[1:]
data = json.loads(Path(source).read_text())
font = ImageFont.truetype('/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf', 15)
bold = ImageFont.truetype('/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf', 15)
palette = {'black':'#0b1118','red':'#e06c75','green':'#98c379','brown':'#e5c07b','yellow':'#e5c07b','blue':'#61afef','magenta':'#c678dd','cyan':'#56b6c2','white':'#d1d5db','brightblack':'#718096','brightwhite':'#ffffff'}
def color(value, background=False):
    if value == 'default': return '#0b1118' if background else '#cbd5e1'
    if len(value)==6 and all(c in '0123456789abcdefABCDEF' for c in value): return '#'+value
    return palette.get(value,'#718096')
cw,ch,pad=10,21,18
image=Image.new('RGB',(data['columns']*cw+2*pad,data['rows']*ch+2*pad),'#0b1118')
draw=ImageDraw.Draw(image)
for y,row in enumerate(data['cells']):
    for x,cell in enumerate(row):
        px,py=pad+x*cw,pad+y*ch
        draw.rectangle((px,py,px+cw-1,py+ch-1),fill=color(cell['bg'],True))
        if cell['text'] in ('▀','▄','█'):
            top=py if cell['text'] != '▄' else py+ch//2
            bottom=py+ch-1 if cell['text'] != '▀' else py+ch//2-1
            draw.rectangle((px,top,px+cw-1,bottom),fill=color(cell['fg']))
        else:
            draw.text((px,py),cell['text'],font=bold if cell['bold'] else font,fill=color(cell['fg']))
image.save(destination)
