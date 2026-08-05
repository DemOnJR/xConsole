import os
import base64
from PIL import Image
from collections import deque

def build_icons():
    # 1. Load original uploaded image
    src_path = r"C:\Users\bogda\.gemini\antigravity-ide\brain\ed87a404-12d1-4930-9172-39a292a5c8da\media__1785898188254.png"
    img = Image.open(src_path).convert("RGBA")
    w, h = img.size

    out = img.copy()

    # 2. Flood fill queue for green background pixels starting from outer corners
    visited = set()
    queue = deque([(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)])

    for pt in list(queue):
        visited.add(pt)

    def is_bg_pixel(x, y):
        r, g, b, a = img.getpixel((x, y))
        g_excess = g - max(r, b)
        return g_excess > 35

    bg_pixels = set()

    while queue:
        x, y = queue.popleft()
        if is_bg_pixel(x, y):
            bg_pixels.add((x, y))
            for dx, dy in [(-1, 0), (1, 0), (0, -1), (0, 1)]:
                nx, ny = x + dx, y + dy
                if 0 <= nx < w and 0 <= ny < h and (nx, ny) not in visited:
                    visited.add((nx, ny))
                    queue.append((nx, ny))

    # 3. Make background pixels transparent, with edge anti-aliasing / desaturation
    for y in range(h):
        for x in range(w):
            if (x, y) in bg_pixels:
                out.putpixel((x, y), (0, 0, 0, 0))
            elif any((x + dx, y + dy) in bg_pixels for dx in [-1, 0, 1] for dy in [-1, 0, 1]):
                # Edge pixel bordering green screen: clean green spill
                r, g, b, a = img.getpixel((x, y))
                g_excess = g - max(r, b)
                if g_excess > 0:
                    g_clean = int(max(r, b))
                    alpha = int(255 * (1.0 - min(1.0, g_excess / 60.0)))
                    out.putpixel((x, y), (r, g_clean, b, alpha))

    # Crop tightly to non-transparent content
    bbox = out.getbbox()
    cropped = out.crop(bbox)

    # 4. Create 1024x1024 master square icon
    master_size = 1024
    target_h = 880
    aspect = cropped.width / cropped.height
    target_w = int(target_h * aspect)

    resized = cropped.resize((target_w, target_h), Image.Resampling.LANCZOS)

    master = Image.new("RGBA", (master_size, master_size), (0, 0, 0, 0))
    offset_x = (master_size - target_w) // 2
    offset_y = (master_size - target_h) // 2

    master.paste(resized, (offset_x, offset_y), resized)

    # Save master icon in workspace
    master_png_path = r"d:\Desktop\xConsole\master_icon.png"
    master.save(master_png_path)
    print(f"Master PNG icon generated at {master_png_path}")

    # Copy to public/icon.png and public/logo.png
    public_dir = r"d:\Desktop\xConsole\public"
    os.makedirs(public_dir, exist_ok=True)
    master.save(os.path.join(public_dir, "icon.png"))
    master.save(os.path.join(public_dir, "logo.png"))

    # Also generate 512x512 PNG for app branding
    master_512 = master.resize((512, 512), Image.Resampling.LANCZOS)
    master_512.save(os.path.join(public_dir, "app-icon.png"))

    # Save SVG version
    with open(master_png_path, "rb") as f:
        b64_data = base64.b64encode(f.read()).decode("utf-8")

    svg_content = f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="100%" height="100%">
  <image href="data:image/png;base64,{b64_data}" x="0" y="0" width="1024" height="1024" />
</svg>
'''
    logo_svg_path = os.path.join(public_dir, "logo.svg")
    with open(logo_svg_path, "w", encoding="utf-8") as f:
        f.write(svg_content)
    print(f"SVG icon generated at {logo_svg_path}")

if __name__ == "__main__":
    build_icons()
