// Renders every `<name>.<lang>.txt` file of a directory with Shiki, for a
// large-scale comparison with oxlight (see `tests/corpus.rs`).
//
// Usage: node corpus.mjs <dir>
// Writes `<dir>/<name>.<lang>__<theme>.html` next to the inputs.

import { readdir, readFile, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import { createHighlighterCore } from 'shiki/core'
import { createOnigurumaEngine } from 'shiki/engine/oniguruma'

const dir = process.argv[2]
const THEMES = ['github-dark', 'dracula', 'one-dark-pro']
const langs = new Map()
for (const name of ['javascript', 'typescript', 'rust', 'php', 'markdown', 'python', 'shellscript', 'json', 'css']) {
  for (const lang of (await import(`@shikijs/langs/${name}`)).default) langs.set(lang.name, lang)
}
const themes = await Promise.all(THEMES.map(async t => (await import(`@shikijs/themes/${t}`)).default))
const highlighter = await createHighlighterCore({
  langs: [...langs.values()],
  themes,
  engine: createOnigurumaEngine(import('shiki/wasm')),
})
for (const file of (await readdir(dir)).filter(f => f.endsWith('.txt'))) {
  const lang = file.split('.')[1]
  const code = await readFile(join(dir, file), 'utf8')
  for (const theme of THEMES) {
    const start = performance.now()
    const html = highlighter.codeToHtml(code, { lang, theme })
    if (theme === THEMES[0]) console.log(`${file}: ${(performance.now() - start).toFixed(1)} ms`)
    await writeFile(join(dir, `${file.slice(0, -4)}__${theme}.html`), html)
  }
}
