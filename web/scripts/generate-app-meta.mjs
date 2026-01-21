import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

function escapeHtml(input) {
	return input
		.replaceAll('&', '&amp;')
		.replaceAll('<', '&lt;')
		.replaceAll('>', '&gt;')
		.replaceAll('"', '&quot;')
		.replaceAll("'", '&#39;');
}

function renderInline(text) {
	let out = escapeHtml(text);

	// `code`
	out = out.replace(/`([^`]+)`/g, (_m, code) => `<code>${code}</code>`);

	// **bold**
	out = out.replace(/\*\*([^*]+)\*\*/g, (_m, bold) => `<strong>${bold}</strong>`);

	// [text](url)
	out = out.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_m, label, url) => {
		const safeUrl = escapeHtml(url);
		return `<a href="${safeUrl}" target="_blank" rel="noreferrer noopener">${label}</a>`;
	});

	return out;
}

function extractVersion(markdown) {
	const cleaned = markdown.replace(/^\uFEFF/, '');
	const match = cleaned.match(/^##\s+(v[^\s]+)\s*/m);
	if (!match) {
		throw new Error('无法从 docs/changelog.md 解析版本号（未找到以 "## v" 开头的标题）');
	}
	return match[1];
}

function markdownToHtml(markdown) {
	const lines = markdown
		.replace(/^\uFEFF/, '')
		.replaceAll('\r\n', '\n')
		.split('\n');

	let html = '';
	let inCodeFence = false;
	let codeBuffer = [];
	let listLevel = -1;
	let openLi = [];
	let inParagraph = false;

	function closeParagraph() {
		if (inParagraph) {
			html += '</p>';
			inParagraph = false;
		}
	}

	function closeLists(targetLevel) {
		while (listLevel > targetLevel) {
			if (openLi[listLevel]) {
				html += '</li>';
			}
			html += '</ul>';
			openLi.pop();
			listLevel -= 1;
		}
	}

	function openLists(targetLevel) {
		while (listLevel < targetLevel) {
			html += '<ul>';
			openLi.push(false);
			listLevel += 1;
		}
	}

	for (const rawLine of lines) {
		const line = rawLine.replace(/\s+$/g, '');

		// ``` fenced code block
		if (line.startsWith('```')) {
			closeParagraph();
			if (inCodeFence) {
				const code = escapeHtml(codeBuffer.join('\n'));
				html += `<pre><code>${code}</code></pre>`;
				codeBuffer = [];
				inCodeFence = false;
			} else {
				closeLists(-1);
				inCodeFence = true;
			}
			continue;
		}

		if (inCodeFence) {
			codeBuffer.push(rawLine);
			continue;
		}

		// blank line
		if (!line) {
			closeParagraph();
			continue;
		}

		// headings
		const headingMatch = line.match(/^(#{1,6})\s+(.*)$/);
		if (headingMatch) {
			closeParagraph();
			closeLists(-1);
			const level = headingMatch[1].length;
			const text = headingMatch[2].trim();
			if (level === 1 && text === '更新记录') {
				continue;
			}
			const tag = `h${Math.min(level, 3)}`;
			html += `<${tag}>${renderInline(text)}</${tag}>`;
			continue;
		}

		// list item
		const listMatch = line.match(/^(\s*)-\s+(.*)$/);
		if (listMatch) {
			closeParagraph();
			const indent = listMatch[1].replaceAll('\t', '  ').length;
			const level = Math.max(0, Math.floor(indent / 2));
			const text = listMatch[2].trim();

			if (level < listLevel) {
				closeLists(level);
			} else if (level > listLevel) {
				openLists(level);
			}

			if (openLi[level]) {
				html += '</li>';
				openLi[level] = false;
			}

			html += `<li>${renderInline(text)}`;
			openLi[level] = true;
			continue;
		}

		// normal paragraph
		closeLists(-1);
		if (!inParagraph) {
			html += '<p>';
			inParagraph = true;
			html += renderInline(line);
		} else {
			html += `<br />${renderInline(line)}`;
		}
	}

	closeParagraph();
	closeLists(-1);

	return html;
}

async function main() {
	const webDir = path.resolve(__dirname, '..');
	const repoRoot = path.resolve(webDir, '..');
	const changelogPath = path.join(repoRoot, 'docs', 'changelog.md');
	const outDir = path.join(webDir, 'src', 'lib', 'generated');
	const versionFile = path.join(outDir, 'app-version.ts');
	const changelogFile = path.join(outDir, 'changelog.ts');

	const markdown = await fs.readFile(changelogPath, 'utf8');
	const version = extractVersion(markdown);
	const changelogHtml = markdownToHtml(markdown);

	await fs.mkdir(outDir, { recursive: true });
	const banner = `// 该文件由 web/scripts/generate-app-meta.mjs 自动生成，请勿手动编辑。\n`;

	await fs.writeFile(
		versionFile,
		`${banner}export const APP_VERSION = ${JSON.stringify(version)};\n`,
		'utf8'
	);

	await fs.writeFile(
		changelogFile,
		`${banner}` +
			`export const CHANGELOG_VERSION = ${JSON.stringify(version)};\n` +
			`export const CHANGELOG_HTML = ${JSON.stringify(changelogHtml)};\n`,
		'utf8'
	);

	console.log(`[generate-app-meta] APP_VERSION=${version}`);
}

main().catch((err) => {
	console.error(`[generate-app-meta] 生成失败: ${err?.stack || err}`);
	process.exit(1);
});
