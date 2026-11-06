/** Столбчатый график по дням: значения над столбцами, подписи снизу (SVG-подобный вид без библиотек). */
export function BarChart({ title, data, format }: { title: string; data: { label: string; value: number }[]; format: (v: number) => string }) {
  const max = Math.max(...data.map((d) => d.value), 0);
  return (
    <figure style={{ margin: '0 0 24px' }}>
      <figcaption className="label">{title}</figcaption>
      <div role="img" aria-label={title} style={{ display: 'flex', alignItems: 'flex-end', gap: 8, height: 150, overflowX: 'auto', paddingTop: 18 }}>
        {data.map((d) => (
          <div key={d.label} style={{ flex: '1 0 44px', display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'flex-end', height: '100%' }}>
            <span style={{ fontSize: 10, color: 'var(--hint)', marginBottom: 2 }}>{format(d.value)}</span>
            <div style={{ width: '70%', height: `${max > 0 ? Math.max((d.value / max) * 100, d.value > 0 ? 3 : 0) : 0}%`, background: 'var(--accent)', borderRadius: '3px 3px 0 0' }} />
          </div>
        ))}
      </div>
      <div style={{ display: 'flex', gap: 8, overflowX: 'auto' }}>
        {data.map((d) => <span key={d.label} style={{ flex: '1 0 44px', textAlign: 'center', fontSize: 10, color: 'var(--hint)', paddingTop: 4 }}>{d.label}</span>)}
      </div>
    </figure>
  );
}
