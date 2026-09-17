/** Relative URLs: Vite proxies these to the Rust server; production serves both on :8765. */

async function json(res) {
  if (!res.ok) {
    const text = await res.text()
    throw new Error(text.trim() || res.statusText)
  }
  if (res.status === 204) return null
  return res.json()
}

export function getSettings() {
  return fetch('/api/settings').then(json)
}

export function patchSettings(body) {
  return fetch('/api/settings', {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  }).then(json)
}

export function listPictures() {
  return fetch('/api/pictures').then(json)
}

export function uploadPicture(file) {
  const form = new FormData()
  form.append('file', file, file.name || 'photo.jpg')
  return fetch('/api/pictures', { method: 'POST', body: form }).then(json)
}

export function deletePicture(id) {
  return fetch(`/api/pictures/${id}`, { method: 'DELETE' }).then(json)
}

export function putRotate(rotate) {
  return fetch('/api/pictures/rotate', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ rotate }),
  }).then(json)
}

/** Convert HEIC/HEIF (or any bitmap) to JPEG via canvas when possible. */
export async function normalizeUpload(file) {
  const type = (file.type || '').toLowerCase()
  const name = (file.name || '').toLowerCase()
  const needsConvert =
    type.includes('heic') ||
    type.includes('heif') ||
    name.endsWith('.heic') ||
    name.endsWith('.heif')

  if (!needsConvert && (type.startsWith('image/') || type === '')) {
    // Try decode; if it works and is already jpeg/png/webp, upload as-is.
    if (type === 'image/jpeg' || type === 'image/png' || type === 'image/webp') {
      return file
    }
  }

  try {
    const bitmap = await createImageBitmap(file)
    const canvas = document.createElement('canvas')
    canvas.width = bitmap.width
    canvas.height = bitmap.height
    const ctx = canvas.getContext('2d')
    ctx.drawImage(bitmap, 0, 0)
    bitmap.close()
    const blob = await new Promise((resolve) =>
      canvas.toBlob(resolve, 'image/jpeg', 0.92),
    )
    if (!blob) return file
    const base = (file.name || 'photo').replace(/\.[^.]+$/, '')
    return new File([blob], `${base}.jpg`, { type: 'image/jpeg' })
  } catch {
    return file
  }
}

export function getDebug() {
  return fetch('/api/debug').then(json)
}

export function getFrameJson() {
  return fetch('/api/frame.json').then(json)
}

export function formatSleep(secs) {
  if (secs < 60) return `${secs}s`
  const m = Math.round(secs / 60)
  if (m < 60) return `${m} min`
  const h = Math.floor(m / 60)
  const rem = m % 60
  return rem ? `${h}h ${rem}m` : `${h}h`
}
