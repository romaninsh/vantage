import { useEffect, useState } from 'react'

const LIMIT = 50

export default function App() {
  const [offset, setOffset] = useState(0)
  const [total, setTotal] = useState(0)
  const [rows, setRows] = useState({})
  const [detail, setDetail] = useState(null)

  // One watch per visible page: the listing arrives instantly from the
  // cache, then augmentation results (rows, latest) stream in as MODIFIED
  // events. Paging away aborts the fetch — the server drops the scenery.
  useEffect(() => {
    setRows({})
    const ctl = new AbortController()
    ;(async () => {
      const res = await fetch(`/api/files?offset=${offset}&limit=${LIMIT}&watch=true`, {
        signal: ctl.signal,
      })
      if (!res.ok) return
      const reader = res.body.getReader()
      const decoder = new TextDecoder()
      let buffer = ''
      for (;;) {
        const { done, value } = await reader.read()
        if (done) break
        buffer += decoder.decode(value, { stream: true })
        let nl
        while ((nl = buffer.indexOf('\n')) >= 0) {
          const line = buffer.slice(0, nl).trim()
          buffer = buffer.slice(nl + 1)
          if (!line) continue
          try {
            const event = JSON.parse(line)
            setRows(rs => ({ ...rs, [event.object.index]: event.object }))
          } catch (e) {
            console.error('failed to parse stream event:', e)
          }
        }
      }
    })().catch(() => {})
    fetch(`/api/files?offset=${offset}&limit=1`, { signal: ctl.signal })
      .then(res => res.json())
      .then(page => setTotal(page.total))
      .catch(() => {})
    return () => ctl.abort()
  }, [offset])

  const open = filename =>
    fetch(`/api/files/${encodeURIComponent(filename)}`)
      .then(res => res.json())
      .then(setDetail)

  const count = Math.max(Math.min(LIMIT, total - offset), Object.keys(rows).length)
  const indexes = Array.from({ length: count }, (_, i) => offset + i)

  return (
    <>
      <h1>GHCN Station Explorer — noaa-ghcn-pds</h1>
      <div className="toolbar">
        <span>{total} files</span>
        <button disabled={offset === 0} onClick={() => setOffset(offset - LIMIT)}>◀ Prev</button>
        <span>{offset}–{Math.min(offset + LIMIT, total || offset + LIMIT)}</span>
        <button disabled={total > 0 && offset + LIMIT >= total} onClick={() => setOffset(offset + LIMIT)}>Next ▶</button>
      </div>
      <table>
        <thead>
          <tr><th>Filename</th><th>Size</th><th>Rows</th><th>Latest</th></tr>
        </thead>
        <tbody>
          {indexes.map(i => {
            const row = rows[i]
            return (
              <tr key={i} onClick={() => row && open(row.filename)}>
                <td>{row ? row.filename : '…'}</td>
                <td className="num">{row?.size ?? ''}</td>
                <td className="num">{row?.rows ?? '…'}</td>
                <td className="num">{row?.latest ?? '…'}</td>
              </tr>
            )
          })}
        </tbody>
      </table>
      {detail && (
        <div className="detail">
          <h2>{detail.Key}</h2>
          <dl>
            {Object.entries(detail).map(([key, value]) => (
              <div key={key}><dt>{key}</dt><dd>{String(value)}</dd></div>
            ))}
          </dl>
          <button onClick={() => setDetail(null)}>close</button>
        </div>
      )}
    </>
  )
}
