// A one-page PDF holding one image: diagram export as PDF (L3). The image is
// raster (RGB, Flate-compressed); with transparency its alpha goes in a soft
// mask. The page is the image's size in points.

async function deflate(data: Uint8Array): Promise<Uint8Array> {
  // "deflate" here is zlib-wrapped, which is what PDF's FlateDecode expects.
  const stream = new Blob([data as BlobPart]).stream().pipeThrough(new CompressionStream('deflate'))
  return new Uint8Array(await new Response(stream).arrayBuffer())
}

/**
 * `rgba` is `w`×`h` pixels (as from `getImageData`); the page is `pageW`×`pageH`
 * points. With `alpha` the transparent parts stay transparent.
 */
export async function imagePdf(rgba: Uint8ClampedArray, w: number, h: number, pageW: number, pageH: number, alpha: boolean): Promise<Uint8Array> {
  const rgb = new Uint8Array(w * h * 3)
  const mask = alpha ? new Uint8Array(w * h) : null
  for (let i = 0; i < w * h; i++) {
    rgb[i * 3] = rgba[i * 4]
    rgb[i * 3 + 1] = rgba[i * 4 + 1]
    rgb[i * 3 + 2] = rgba[i * 4 + 2]
    if (mask) mask[i] = rgba[i * 4 + 3]
  }
  const [image, soft] = await Promise.all([deflate(rgb), mask ? deflate(mask) : null])
  const pw = pageW.toFixed(2)
  const ph = pageH.toFixed(2)

  const enc = new TextEncoder()
  const parts: Uint8Array[] = []
  const offsets: number[] = []
  let size = 0
  const push = (p: Uint8Array | string) => {
    const b = typeof p === 'string' ? enc.encode(p) : p
    parts.push(b)
    size += b.length
  }
  const obj = (n: number, dict: string, stream?: Uint8Array) => {
    offsets[n] = size
    push(`${n} 0 obj\n${dict}\n`)
    if (stream) {
      push('stream\n')
      push(stream)
      push('\nendstream\n')
    }
    push('endobj\n')
  }

  push('%PDF-1.4\n')
  const content = enc.encode(`q ${pw} 0 0 ${ph} 0 0 cm /Im0 Do Q`)
  obj(1, '<< /Type /Catalog /Pages 2 0 R >>')
  obj(2, '<< /Type /Pages /Kids [3 0 R] /Count 1 >>')
  obj(3, `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${pw} ${ph}] /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>`)
  obj(4, `<< /Type /XObject /Subtype /Image /Width ${w} /Height ${h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode${soft ? ' /SMask 6 0 R' : ''} /Length ${image.length} >>`, image)
  obj(5, `<< /Length ${content.length} >>`, content)
  if (soft) obj(6, `<< /Type /XObject /Subtype /Image /Width ${w} /Height ${h} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length ${soft.length} >>`, soft)
  const count = soft ? 7 : 6
  const xref = size
  // Each cross-reference entry is exactly 20 bytes.
  const entries = offsets.slice(1, count).map((o) => `${String(o).padStart(10, '0')} 00000 n \n`)
  push(`xref\n0 ${count}\n0000000000 65535 f \n${entries.join('')}trailer\n<< /Size ${count} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`)

  const out = new Uint8Array(size)
  let at = 0
  for (const p of parts) {
    out.set(p, at)
    at += p.length
  }
  return out
}
