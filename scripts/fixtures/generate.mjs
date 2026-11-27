// Generates oxlight's golden fixtures with the reference Shiki implementation.
//
// Usage: npm install && npm run generate
//
// Outputs (relative to the repository root):
// - tests/fixtures/langs/<name>.json    Shiki language registrations
// - tests/fixtures/themes/<name>.json   Shiki theme registrations
// - tests/fixtures/expected/<sample>__<theme>.html  output of `codeToHtml`
// - tests/fixtures/LICENSES.md          provenance and licenses of grammars/themes

import { mkdir, readdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createHighlighterCore } from 'shiki/core'
import { createOnigurumaEngine } from 'shiki/engine/oniguruma'
import { grammars } from 'tm-grammars'
import { themes as themeInfos } from 'tm-themes'

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..')
const fixtures = join(root, 'tests', 'fixtures')

const LANGS = ['javascript', 'typescript', 'rust', 'php', 'markdown', 'python', 'shellscript', 'json', 'css']
const THEMES = ['github-dark', 'github-light', 'dracula', 'catppuccin-mocha', 'one-dark-pro', 'vitesse-light', 'nord', 'min-light']

const langRegs = new Map()
for (const name of LANGS) {
  const mod = await import(`@shikijs/langs/${name}`)
  for (const lang of mod.default) langRegs.set(lang.name, lang)
}
const themeRegs = new Map()
for (const name of THEMES) {
  const mod = await import(`@shikijs/themes/${name}`)
  themeRegs.set(name, mod.default)
}

const highlighter = await createHighlighterCore({
  langs: [...langRegs.values()],
  themes: [...themeRegs.values()],
  engine: createOnigurumaEngine(import('shiki/wasm')),
})

await mkdir(join(fixtures, 'langs'), { recursive: true })
await mkdir(join(fixtures, 'themes'), { recursive: true })
await mkdir(join(fixtures, 'expected'), { recursive: true })

for (const [name, lang] of langRegs)
  await writeFile(join(fixtures, 'langs', `${name}.json`), `${JSON.stringify(lang)}\n`)
for (const [name, theme] of themeRegs)
  await writeFile(join(fixtures, 'themes', `${name}.json`), `${JSON.stringify(theme)}\n`)

const samples = (await readdir(join(fixtures, 'samples'))).filter(f => f.endsWith('.txt')).sort()
for (const file of samples) {
  const [base, lang] = file.split('.')
  const code = await readFile(join(fixtures, 'samples', file), 'utf8')
  for (const theme of THEMES) {
    const html = highlighter.codeToHtml(code, { lang, theme })
    await writeFile(join(fixtures, 'expected', `${base}.${lang}__${theme}.html`), html)
  }
  const dual = highlighter.codeToHtml(code, { lang, themes: { light: 'github-light', dark: 'github-dark' } })
  await writeFile(join(fixtures, 'expected', `${base}.${lang}__dual.html`), dual)
}

const lines = [
  '# Fixture licenses',
  '',
  'The grammars and themes in this folder are test fixtures, copied from the',
  `[Shiki](https://shiki.style) bundles (\`@shikijs/langs\` and \`@shikijs/themes\` ${(await import('shiki/package.json', { with: { type: 'json' } })).default.version}),`,
  'which redistribute them from [tm-grammars](https://github.com/shikijs/textmate-grammars-themes).',
  'Each one keeps its original license, listed below.',
  '',
  '| Kind | Name | License | Source |',
  '| --- | --- | --- | --- |',
]
for (const name of [...langRegs.keys()].sort()) {
  const info = grammars.find(g => g.name === name)
  lines.push(`| grammar | ${name} | ${info?.license ?? 'unknown'} | ${info?.source ?? ''} |`)
}
for (const name of [...themeRegs.keys()].sort()) {
  const info = themeInfos.find(t => t.name === name)
  lines.push(`| theme | ${name} | ${info?.license ?? 'unknown'} | ${info?.source ?? ''} |`)
}
lines.push('')
await writeFile(join(fixtures, 'LICENSES.md'), lines.join('\n'))

console.log(`Wrote ${langRegs.size} languages, ${themeRegs.size} themes, ${samples.length} samples.`)
