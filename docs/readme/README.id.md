<div align="center">
  <img src="../../.github/assets/logo.png" width="112" height="112" alt="Zephium" />
  <h1>Zephium</h1>

  <p><strong>Browser dan lingkungan kerja yang cepat dan lengkap fiturnya,<br />dibangun ulang untuk Anda dan agen Anda.</strong></p>

  <p>
    <a href="https://github.com/zephium-browser/Zephium/releases"><img src="https://img.shields.io/github/v/release/zephium-browser/Zephium?include_prereleases&label=release&color=blue" alt="Rilis terbaru" /></a>
    <a href="../../LICENSE"><img src="https://img.shields.io/badge/license-MPL--2.0-blue" alt="Lisensi: MPL-2.0" /></a>
    <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey" alt="Platform: macOS dan Windows" />
    <a href="https://discord.gg/tyveTUyEp7"><img src="https://img.shields.io/badge/Discord-5865F2?logo=discord&logoColor=white" alt="Discord" /></a>
    <a href="https://www.youtube.com/@crynta"><img src="https://img.shields.io/badge/YouTube-FF0000?logo=youtube&logoColor=white" alt="YouTube" /></a>
  </p>
</div>

<details align="center">
  <summary><sub>Baca dalam bahasa lain</sub></summary>
  <sub>
    <a href="../../README.md">English</a> ·
    <a href="README.zh-CN.md">简体中文</a> ·
    <a href="README.ja.md">日本語</a> ·
    <a href="README.ko.md">한국어</a> ·
    <a href="README.hi.md">हिन्दी</a> ·
    <a href="README.es.md">Español</a> ·
    <a href="README.pt-BR.md">Português</a> ·
    <a href="README.fr.md">Français</a> ·
    <a href="README.de.md">Deutsch</a> ·
    <a href="README.pl.md">Polski</a> ·
    <a href="README.ru.md">Русский</a>
  </sub>
</details>

<p align="center">
  <img src="../../.github/assets/browse.webp" alt="Zephium dalam mode Browse, dengan tab di bilah samping dan zephium.app terbuka" width="960" />
</p>

<p align="center">
  <strong>Efisiensi Safari. Perlindungan Brave. Desain Arc.</strong><br />
  Ditambah <strong>Work</strong>, kanvas tempat agen Anda mengerjakan tugas nyata di depan mata Anda.
</p>

---

Zephium adalah browser sumber terbuka yang dibangun dengan Rust di atas mesin web
bawaan sistem operasi Anda: WebKit di macOS, mesin yang dipakai Safari, dan
WebView2 di Windows. Zephium bukan fork dari Chromium atau Firefox. Zephium
memblokir iklan dan pelacak secara native, menjalankan ekstensi Chrome, dan
menyediakan tugas, catatan, serta waktu Anda di web hanya dengan satu klik.
Ukuran unduhannya sekitar 30 MB.

Satu sakelar dari situ ada **Work**. Pekerjaan Anda sudah berlangsung di
browser, tempat tab, akun login, dan riwayat Anda berada. Work membawa agen ke
sana, bukan menyuruh Anda pindah ke tempat lain, dan memungkinkan Anda
menyaksikan mereka bekerja.

> [!NOTE]
> Zephium sedang dalam tahap beta dan siap menjadi browser harian Anda. Jika ada yang tidak beres, silakan [buka issue](https://github.com/zephium-browser/Zephium/issues).

## Work

Jelaskan hasil yang Anda inginkan dengan kata-kata sendiri: merencanakan
perjalanan, membandingkan vendor, merancang sistem, memperbaiki bug. Work
mengerjakannya di atas kanvas, dan Anda melihat setiap langkahnya.

<p align="center">
  <img src="../../.github/assets/work.webp" alt="Zephium dalam mode Work: agen membandingkan AWS, Vercel, Hetzner, dan Cloudflare lalu menggambar arsitektur referensi" width="960" />
</p>
<p align="center"><sub>Diminta membandingkan hosting untuk AI SaaS, Work membaca halaman harga, merekomendasikan stack, dan menggambar arsitekturnya.</sub></p>

- **Dimulai dari apa yang sudah Anda punya.** Work mengingat apa yang ia ketahui
  tentang Anda, menelusuri riwayat Anda saat berguna, dan memuat skill yang
  tepat untuk pekerjaan itu. Setiap langkah muncul di kanvas saat terjadi.
- **Para asisten bekerja paralel, secara terbuka.** Untuk sebuah perjalanan,
  satu asisten mencari tempat tinggal, yang lain memeriksa visa, dan yang ketiga
  membandingkan penerbangan. Mereka menjelajahi halaman langsung yang bisa Anda
  tonton, dan setiap sumber terbuka di panel di samping kanvas.
- **Hasil tetap ada di kanvas.** Perbandingan, tabel, grafik, diagram, rencana,
  kode, dan dokumen ditata agar mudah dibaca. Semuanya tidak hilang di gulungan
  obrolan.
- **Rencana menjadi hari Anda.** Satu klik mengubah setiap langkah rencana
  menjadi tugas yang terhubung kembali ke Work tersebut. Apa pun yang layak disimpan
  bisa disimpan sebagai catatan.
- **Jangkauannya melampaui browser.** Work membaca dan mengedit folder yang Anda
  izinkan, menjalankan perintah, dan menyerahkan pekerjaan coding yang lebih
  besar ke Claude Code atau Codex. Work terhubung ke server MCP seperti Linear,
  Notion, Sentry, Stripe, dan Figma, serta ke alat baris perintah seperti `gh`.
- **Anda tetap memegang kendali.** Apa pun yang memposting, menggabungkan, atau
  mengubah sesuatu menunggu persetujuan Anda.
- **Pakai model apa saja.** Gunakan kunci Anda sendiri untuk Anthropic, OpenAI,
  Google Gemini, DeepSeek, atau OpenRouter, atau jalankan model secara lokal
  lewat Ollama, LM Studio, atau server mana pun yang kompatibel dengan OpenAI.
  Kunci disimpan di keychain sistem Anda.

Work hadir dengan 25 skill, termasuk perencanaan perjalanan, riset,
membandingkan dan memilih, merencanakan hari saya, memperbaiki bug, dan status
mingguan. Anda bisa menulis skill sendiri.

## Browse

### Cepat dan ringan

- WebKit native di macOS dan WebView2 di Windows, sehingga halaman berjalan di
  mesin yang sudah selalu diperbarui oleh sistem Anda.
- Tab yang tidak aktif tertidur dan bangun saat Anda kembali, sehingga memori
  tetap hemat meski banyak tab terbuka.
- Pemblokir iklan dan pelacak native, aktif secara default, dibangun di atas
  [adblock-rust](https://github.com/brave/adblock-rust) milik Brave dengan
  EasyList dan EasyPrivacy. Permintaan dihentikan sebelum halaman sempat
  mengirimnya, dan Anda bisa menyembunyikan elemen lain di halaman dengan satu
  klik.

### Dirancang untuk ditinggali

- Tab vertikal di bilah samping yang tenang, dengan space, profil, tab yang
  disematkan, folder, dan tampilan terpisah.
- Liquid Glass di macOS 26 dan Mica di Windows 11.
- Launcher di `⌘ ⇧ Space` (`Ctrl Shift Space` di Windows) menjangkau tab,
  riwayat, catatan, dan perintah dari mana saja di desktop Anda. Apa pun yang
  Anda ketik di sana bisa menjadi tugas.
- Setiap pintasan keyboard bisa diubah.

### Ekstensi

Pasang ekstensi langsung dari Chrome Web Store. Dua puluh ekstensi populer telah
diverifikasi berjalan baik di Zephium dan tersedia dengan satu klik di manajer
ekstensi, termasuk 1Password, Bitwarden, Grammarly, DeepL, Dark Reader, Vimium,
SponsorBlock, Raindrop.io, Notion Web Clipper, dan Refined GitHub.

### Bawa semuanya bersama Anda

Alur sambutan mengimpor dari Chrome, Safari, Arc, Zen, Firefox, Brave, dan
Edge.

## Bawaan

<table>
  <tr>
    <td width="33%" align="center"><img src="../../.github/assets/tasks.webp" alt="Tasks: sebuah tugas dengan status, tenggat, daftar, prioritas, dan halaman yang tertaut" /></td>
    <td width="33%" align="center"><img src="../../.github/assets/time.webp" alt="Time: 42 menit di web hari ini, pengatur waktu fokus, dan waktu per situs" /></td>
    <td width="33%" align="center"><img src="../../.github/assets/notes.webp" alt="Notes: catatan Markdown dengan judul, daftar, dan kode di samping halaman" /></td>
  </tr>
  <tr>
    <td valign="top"><strong>Tasks</strong><br /><sub>Tulis seperti Anda mengucapkannya, misalnya "telepon Anna besok jam 3". Ada daftar, prioritas, tenggat, subtugas, dan halaman yang sedang Anda buka tetap tertaut.</sub></td>
    <td valign="top"><strong>Time</strong><br /><sub>Lihat ke mana waktu Anda di web terpakai, dihitung hanya di perangkat ini. Mulai satu putaran Focus dan situs yang Anda pilih tetap tertutup sampai istirahat.</sub></td>
    <td valign="top"><strong>Notes</strong><br /><sub>Catatan di samping halaman, masing-masing berupa file Markdown milik Anda. Perubahan dari aplikasi lain muncul di Zephium.</sub></td>
  </tr>
</table>

## Privat secara default

- **Tanpa telemetri.** Zephium tidak mengumpulkan atau mengirim apa pun tentang cara Anda menjelajah.
- **Tanpa akun.** Riwayat, tugas, catatan, memori, dan waktu tersimpan di
  perangkat Anda.
- **Jendela privat tidak menyimpan apa pun** setelah ditutup.

## Unduh

| Platform | Arsitektur | Installer | Persyaratan |
| -------- | ---------- | --------- | ----------- |
| macOS | Apple Silicon | [`Zephium-macOS-arm64.dmg`](https://github.com/zephium-browser/Zephium/releases/latest/download/Zephium-macOS-arm64.dmg) | macOS Sonoma 14 atau lebih baru |
| Windows | x64 | [`Zephium-Windows-x64-setup.exe`](https://github.com/zephium-browser/Zephium/releases/latest/download/Zephium-Windows-x64-setup.exe) | Windows 10 atau 11 |

Build Windows belum ditandatangani kode, sehingga SmartScreen mungkin menampilkan
"Windows protected your PC". Pilih **More info**, lalu **Run anyway**.
Penandatanganan sedang disiapkan.

Zephium mengunduh pembaruan di latar belakang dan memasangnya saat Anda memilih
**Relaunch to update**. Semua rilis tersedia di
[halaman rilis](https://github.com/zephium-browser/Zephium/releases).

## Rencana berikutnya

- Linux dan Mac Intel.
- Build Windows yang ditandatangani kode.
- Lebih banyak ekstensi yang terverifikasi berfungsi, dan ekstensi native yang tertanam di Zephium.
- Dan masih banyak lagi.

## Build dari sumber

Prasyarat:

- [Rust](https://rustup.rs) lewat `rustup`. Toolchain yang dipatok di
  [`rust-toolchain.toml`](../../rust-toolchain.toml) terpasang sendiri saat
  pertama kali dipakai.
- Node.js, dengan versi seperti di [`.node-version`](../../.node-version).
- pnpm, lewat Corepack: `corepack enable`.
- [Prasyarat Tauri](https://v2.tauri.app/start/prerequisites/) untuk platform
  Anda: Xcode Command Line Tools di macOS, atau Microsoft C++ Build Tools dan
  WebView2 di Windows.

```sh
git clone https://github.com/zephium-browser/Zephium.git
cd Zephium
pnpm install --frozen-lockfile
pnpm dev
```

Build pengembangan memakai profil `app.zephium.dev` tersendiri, sehingga tidak
menyentuh data Zephium yang terpasang. Linux belum didukung. Baca
[CONTRIBUTING.md](../../CONTRIBUTING.md) sebelum membuka pull request.

## Arsitektur

- Halaman yang tidak tepercaya berjalan di WebView native mentah, terpisah dari
  antarmuka Svelte berhak istimewa yang menggambar browser.
- Inti Rust memegang tab, penyimpanan, pemblokir, ekstensi, dan agen.
- Setiap profil punya data situs web yang terisolasi sendiri, serta data
  riwayat, sesi, dan favicon milik Zephium.
- Pemblokir jaringan bersifat native dan dibangun di atas
  [adblock-rust](https://github.com/brave/adblock-rust) milik Brave.
- Adaptor sempit dalam pohon kode untuk Tauri dan Wry menangani kebijakan
  penyimpanan, pembuatan WebView, callback, dan penutupan.

Baca selengkapnya di [docs/architecture.md](../architecture.md) dan
[docs/security-model.md](../security-model.md).

## Berkontribusi

Zephium dikelola oleh satu orang dengan arah produk yang jelas, jadi mohon
diskusikan perubahan yang lebih besar sebelum menulisnya.
[CONTRIBUTING.md](../../CONTRIBUTING.md) menjelaskan apa yang digabungkan dan
bagaimana caranya. Pertanyaan dan ide dipersilakan di
[Discord](https://discord.gg/tyveTUyEp7).

## Keamanan

Laporkan kerentanan secara privat, bukan di issue publik. Lihat
[SECURITY.md](../../SECURITY.md).

## Lisensi

Zephium berlisensi [Mozilla Public License 2.0](../../LICENSE).

Daftar filter EasyList dan EasyPrivacy yang disertakan digunakan berdasarkan
[CC BY-SA 3.0](../../assets/blocker-seed/v1/LICENSE-CC-BY-SA-3.0.txt); lihat
[pemberitahuan](../../assets/blocker-seed/v1/NOTICE) untuk atribusi.

Nama dan logo Zephium tidak dilisensikan di bawah MPL.

## Ucapan terima kasih

Zephium berdiri di atas [Tauri](https://tauri.app), [Wry](https://github.com/tauri-apps/wry),
[adblock-rust](https://github.com/brave/adblock-rust) milik Brave, dan
[EasyList](https://easylist.to).
