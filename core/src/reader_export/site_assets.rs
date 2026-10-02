pub(super) const CSS: &str = r#"
:root{color-scheme:light;--ink:#183446;--muted:#596d7a;--paper:#f8f6f0;--line:#dbe0df;--accent:#245f75}
*{box-sizing:border-box}body{margin:0;background:var(--paper);color:var(--ink);font:1rem/1.8 system-ui,sans-serif}
header,main,footer{max-width:74rem;margin:auto;padding:1.4rem 2rem}header{border-bottom:1px solid var(--line)}
.site-title{font-size:1.3rem;font-weight:700;text-decoration:none}nav{display:flex;gap:1.2rem;flex-wrap:wrap;margin-top:.8rem}
a{color:var(--accent);text-underline-offset:.2em}a:hover{color:#073248}a:focus-visible,button:focus-visible,input:focus-visible,select:focus-visible{outline:3px solid #d2a950;outline-offset:3px}
main{min-height:70vh}h1{font-size:clamp(1.8rem,4vw,2.8rem);line-height:1.25;margin:1rem 0 1.6rem}h2{font-size:1.3rem;margin-top:2rem}
p,dd,li{overflow-wrap:anywhere}p{white-space:pre-wrap}.lede{font-size:1.15rem;color:var(--muted)}
.category-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(12rem,1fr));gap:1rem;margin:2rem 0}.category{display:flex;flex-direction:column;background:#fff;border:1px solid var(--line);border-radius:.5rem;padding:1.2rem;text-decoration:none}.category span{color:var(--muted);font-size:.9rem}
.page-list{list-style:none;padding:0}.page-list li{border-bottom:1px solid var(--line);padding:.7rem 0}.unavailable{color:var(--muted);font-style:italic}
dl{display:grid;grid-template-columns:minmax(6rem,10rem) 1fr;gap:.8rem;border-bottom:1px solid var(--line);padding:.7rem 0;margin:0}dt{font-weight:600}dd{margin:0;white-space:pre-wrap}
.choice{border-left:3px solid #be9a57;background:#fff8e8;margin:1rem 0;padding:.7rem 1rem}.relation-graph{width:100%;height:auto;max-height:40rem;border:1px solid var(--line)}
input,select,button{font:inherit;border:1px solid #a8b8bd;border-radius:.35rem;background:white;color:var(--ink);padding:.5rem .8rem}button{cursor:pointer}.search-controls{display:flex;gap:1.2rem;flex-wrap:wrap}.search-controls label{display:flex;gap:.6rem;align-items:center}#query{min-width:16rem}#results{list-style:none;padding:0}#results li{border-bottom:1px solid var(--line);padding:1rem 0}#results p{color:var(--muted);margin:.5rem 0}
.map-controls{display:flex;gap:.6rem;flex-wrap:wrap;margin:1rem 0}.reader-map{touch-action:none;background:white;display:block}.map-details{padding-left:1.5rem}footer{color:var(--muted);border-top:1px solid var(--line);font-size:.9rem}
@media(max-width:640px){header,main,footer{padding:1rem}nav{gap:.8rem}dl{grid-template-columns:1fr;gap:.2rem}.search-controls label{display:block}#query{min-width:0;width:100%}.search-controls{display:block}.search-controls label{margin-bottom:1rem}}
"#;

pub(super) const JS: &str = r#"(() => {
const input = document.getElementById('query');
const kind = document.getElementById('kind');
const list = document.getElementById('results');
const status = document.getElementById('search-status');
if (input && list) {
  const update = () => {
    const query = input.value.trim().toLocaleLowerCase();
    const selectedKind = kind ? kind.value : '';
    list.replaceChildren();
    let count = 0;
    if (query || selectedKind) for (const entry of (window.READER_SEARCH_DATA || [])) {
      if (selectedKind && entry.kind !== selectedKind) continue;
      const source = [entry.title, ...(entry.aliases || []), entry.text].join(' ');
      const position = source.toLocaleLowerCase().indexOf(query);
      if (query && position < 0) continue;
      const item = document.createElement('li');
      const link = document.createElement('a');
      link.href = entry.url;
      link.textContent = entry.title;
      const excerpt = document.createElement('p');
      const start = Math.max(0, position - 35);
      excerpt.textContent = (start ? '…' : '') + source.slice(start, start + 180) + (source.length > start + 180 ? '…' : '');
      item.append(link, excerpt);
      list.append(item);
      count++;
    }
    if (status) status.textContent = query || selectedKind ? `${count} 项公开结果` : '输入正文、别名或公开字段，也可以按类型筛选';
  };
  input.addEventListener('input', update);
  if (kind) kind.addEventListener('change', update);
  update();
}
for (const svg of document.querySelectorAll('svg.reader-map')) {
  const original = svg.getAttribute('viewBox').split(/\s+/).map(Number);
  let view = original.slice();
  const apply = () => svg.setAttribute('viewBox', view.join(' '));
  const controls = svg.previousElementSibling;
  if (controls) for (const button of controls.querySelectorAll('button[data-map-action]')) {
    button.addEventListener('click', () => {
      if (button.dataset.mapAction === 'reset') view = original.slice();
      else {
        const factor = button.dataset.mapAction === 'in' ? 0.8 : 1.25;
        const width = view[2] * factor;
        const height = view[3] * factor;
        if (width < original[2] / 100 || width > original[2] * 100) return;
        view = [view[0] + (view[2] - width) / 2, view[1] + (view[3] - height) / 2, width, height];
      }
      apply();
    });
  }
  let drag = null;
  svg.addEventListener('pointerdown', event => {
    if (event.target.closest('a')) return;
    drag = [event.clientX, event.clientY, view.slice()];
    svg.setPointerCapture(event.pointerId);
  });
  svg.addEventListener('pointermove', event => {
    if (!drag) return;
    const rect = svg.getBoundingClientRect();
    view = [drag[2][0] - (event.clientX - drag[0]) / rect.width * drag[2][2], drag[2][1] - (event.clientY - drag[1]) / rect.height * drag[2][3], drag[2][2], drag[2][3]];
    apply();
  });
  svg.addEventListener('pointerup', () => { drag = null; });
  svg.addEventListener('pointercancel', () => { drag = null; });
}
})();
"#;
