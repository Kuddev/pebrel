import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import * as esbuild from 'esbuild';

const root = path.dirname(fileURLToPath(import.meta.url));
const assets = path.resolve(root, '../../app/src/main/assets/reader');
const upstream = fs.readFileSync(path.join(root, 'node_modules/mermaid/dist/mermaid.min.js'), 'utf8');
// 保留 Mermaid 的完整图表实现，只降低语法目标；不依赖设备恰好更新过 WebView。
const transformed = esbuild.transformSync(upstream, {
  target: 'chrome91', minify: true, legalComments: 'inline', charset: 'utf8',
  // 雷电 WebView 91 实测不支持 static {}，不能只凭 UA 采用 esbuild 的版本推断。
  supported: { 'class-static-blocks': false },
});
fs.writeFileSync(path.join(assets, 'mermaid/mermaid.min.js'), transformed.code, 'utf8');
esbuild.buildSync({
  entryPoints: [path.join(root, 'compat.js')], outfile: path.join(assets, 'reader-compat.js'),
  bundle: true, platform: 'browser', format: 'iife', target: 'chrome91',
  minify: true, legalComments: 'inline', charset: 'utf8',
});
fs.copyFileSync(path.join(root, 'node_modules/core-js/LICENSE'), path.resolve(root, '../licenses/core-js-MIT.txt'));
const digest = data => createHash('sha256').update(data).digest('hex');
const manifestPath = path.join(assets, 'extensions-vendor.json');
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
const mermaid = manifest.find(entry => entry.package === 'mermaid');
mermaid.transform = { esbuild: '0.28.2', target: 'chrome91',
  supported: { 'class-static-blocks': false }, upstream_sha256: digest(upstream) };
mermaid.files['mermaid/mermaid.min.js'] = digest(transformed.code);
const compatibility = { package: 'core-js', version: '3.50.0', license: 'MIT',
  source: 'https://registry.npmjs.org/core-js/-/core-js-3.50.0.tgz',
  modules: ['object.has-own', 'array.at', 'string.at', 'structured-clone'],
  files: { 'reader-compat.js': digest(fs.readFileSync(path.join(assets, 'reader-compat.js'))) } };
const index = manifest.findIndex(entry => entry.package === 'core-js');
if (index < 0) manifest.push(compatibility); else manifest[index] = compatibility;
fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n', 'utf8');
