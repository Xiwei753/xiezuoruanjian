# 素笺写作官网

这是“素笺写作”的官网静态目录，官网入口文件是 `site/index.html`。

下载链接集中维护在 `site/download.json`。发版后，只需要手动修改其中的 GitHub、Gitee、百度网盘、蓝奏云、夸克网盘和 123 云盘链接；空链接会在页面上显示为“暂未提供”。

请不要把 APK、EXE、AppImage、HAP 等安装包提交到 `site/` 目录。安装包仍应放在 GitHub Releases、Gitee Releases 或网盘中。

Cloudflare Pages 自动部署由 `.github/workflows/deploy-site-cloudflare.yml` 完成。仓库的 GitHub Actions Secrets 需要配置：

- `CLOUDFLARE_ACCOUNT_ID`
- `CLOUDFLARE_API_TOKEN`

Cloudflare API Token 使用最小权限：`Account → Cloudflare Pages → Edit`。
