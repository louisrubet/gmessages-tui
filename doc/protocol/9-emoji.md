# 9. Appendix: pairing emoji

Google account pairing shows one emoji, chosen by index in a list
([4](4-pairing.md#3-handshake)): `list[n mod len(list)]`. The list depends on the emoji version
the phone confirms. The **order** is what matters: the index must pick the same emoji as the
phone.

Each entry below is a single code point, written without variation selector (`U+FE0F`), except
three sequences joined by `U+200D`:

| Emoji | Code points |
|---|---|
| 😮‍💨 | U+1F62E U+200D U+1F4A8 |
| 🍋‍🟩 | U+1F34B U+200D U+1F7E9 |
| 🐦‍🔥 | U+1F426 U+200D U+1F525 |

Rows give the index of their first entry; entries are separated by spaces.

## Version 0 (305 entries)

Note that 🍞 appears twice (indices 133 and 288): keep both, the length is 305.

| Index | Emoji |
|---|---|
| 0 | 😁 😅 🤣 🫠 🥰 😇 🤩 😘 😜 🤗 |
| 10 | 🤔 🤐 😴 🥶 🤯 🤠 🥳 🥸 😎 🤓 |
| 20 | 🧐 🥹 😭 😱 😖 🥱 😮‍💨 🤡 💩 👻 |
| 30 | 👽 🤖 😻 💌 💘 💕 ❤ 💢 💥 💫 |
| 40 | 💬 🗯 💤 👋 🙌 🙏 ✍ 🦶 👂 🧠 |
| 50 | 🦴 👀 🧑 🧚 🧍 👣 🐵 🐶 🐺 🦊 |
| 60 | 🦁 🐯 🦓 🦄 🐑 🐮 🐷 🐿 🐰 🦇 |
| 70 | 🐻 🐨 🐼 🦥 🐾 🐔 🐥 🐦 🕊 🦆 |
| 80 | 🦉 🪶 🦩 🐸 🐢 🦎 🐍 🐳 🐬 🦭 |
| 90 | 🐠 🐡 🦈 🪸 🐌 🦋 🐛 🐝 🐞 🪱 |
| 100 | 💐 🌸 🌹 🌻 🌱 🌲 🌴 🌵 🌾 ☘ |
| 110 | 🍁 🍂 🍄 🪺 🍇 🍈 🍉 🍋 🍌 🍍 |
| 120 | 🍎 🍐 🍒 🍓 🥝 🥥 🥑 🥕 🌽 🌶 |
| 130 | 🫑 🥦 🥜 🍞 🥐 🥨 🧀 🍗 🍔 🍟 |
| 140 | 🍕 🌭 🌮 🥗 🥣 🍿 🦀 🦑 🍦 🍩 |
| 150 | 🍪 🍫 🍰 🍬 🍭 ☕ 🫖 🍹 🥤 🧊 |
| 160 | 🥢 🍽 🥄 🧭 🏔 🌋 🏕 🏖 🪵 🏗 |
| 170 | 🏡 🏰 🛝 🚂 🛵 🛴 🛼 🚥 ⚓ 🛟 |
| 180 | ⛵ ✈ 🚀 🛸 🧳 ⏰ 🌙 🌡 🌞 🪐 |
| 190 | 🌠 🌧 🌀 🌈 ☂ ⚡ ❄ ⛄ 🔥 🎇 |
| 200 | 🧨 ✨ 🎈 🎉 🎁 🏆 🏅 ⚽ ⚾ 🏀 |
| 210 | 🏐 🏈 🎾 🎳 🏓 🥊 ⛳ ⛸ 🎯 🪁 |
| 220 | 🔮 🎮 🧩 🧸 🪩 🖼 🎨 🧵 🧶 🦺 |
| 230 | 🧣 🧤 🧦 🎒 🩴 👟 👑 👒 🎩 🧢 |
| 240 | 💎 🔔 🎤 📻 🎷 🪗 🎸 🎺 🎻 🥁 |
| 250 | 📺 🔋 💻 💿 ☎ 🕯 💡 📖 📚 📬 |
| 260 | ✏ ✒ 🖌 🖍 📝 💼 📋 📌 📎 🔑 |
| 270 | 🔧 🧲 🪜 🧬 🔭 🩹 🩺 🪞 🛋 🪑 |
| 280 | 🛁 🧹 🧺 🔱 🏁 🐪 🐘 🦃 🍞 🍜 |
| 290 | 🍠 🚘 🤿 🃏 👕 📸 🏷 ✂ 🧪 🚪 |
| 300 | 🧴 🧻 🪣 🧽 🚸 |

## Version 1 (308 entries)

Derived from version 0:

1. remove the duplicates, keeping the first occurrence (the second 🍞 goes);
2. append, in this order: 🍋‍🟩 🐦‍🔥 🐲 🪅 🦜 🏺 🗿 🫐 ⛽ 🍱 🥡 🧋 🍼 📐;
3. remove: 💻 🤗 💬 👋 😁 😎 😇 🥰 🤓 🤩.

Result:

| Index | Emoji |
|---|---|
| 0 | 😅 🤣 🫠 😘 😜 🤔 🤐 😴 🥶 🤯 |
| 10 | 🤠 🥳 🥸 🧐 🥹 😭 😱 😖 🥱 😮‍💨 |
| 20 | 🤡 💩 👻 👽 🤖 😻 💌 💘 💕 ❤ |
| 30 | 💢 💥 💫 🗯 💤 🙌 🙏 ✍ 🦶 👂 |
| 40 | 🧠 🦴 👀 🧑 🧚 🧍 👣 🐵 🐶 🐺 |
| 50 | 🦊 🦁 🐯 🦓 🦄 🐑 🐮 🐷 🐿 🐰 |
| 60 | 🦇 🐻 🐨 🐼 🦥 🐾 🐔 🐥 🐦 🕊 |
| 70 | 🦆 🦉 🪶 🦩 🐸 🐢 🦎 🐍 🐳 🐬 |
| 80 | 🦭 🐠 🐡 🦈 🪸 🐌 🦋 🐛 🐝 🐞 |
| 90 | 🪱 💐 🌸 🌹 🌻 🌱 🌲 🌴 🌵 🌾 |
| 100 | ☘ 🍁 🍂 🍄 🪺 🍇 🍈 🍉 🍋 🍌 |
| 110 | 🍍 🍎 🍐 🍒 🍓 🥝 🥥 🥑 🥕 🌽 |
| 120 | 🌶 🫑 🥦 🥜 🍞 🥐 🥨 🧀 🍗 🍔 |
| 130 | 🍟 🍕 🌭 🌮 🥗 🥣 🍿 🦀 🦑 🍦 |
| 140 | 🍩 🍪 🍫 🍰 🍬 🍭 ☕ 🫖 🍹 🥤 |
| 150 | 🧊 🥢 🍽 🥄 🧭 🏔 🌋 🏕 🏖 🪵 |
| 160 | 🏗 🏡 🏰 🛝 🚂 🛵 🛴 🛼 🚥 ⚓ |
| 170 | 🛟 ⛵ ✈ 🚀 🛸 🧳 ⏰ 🌙 🌡 🌞 |
| 180 | 🪐 🌠 🌧 🌀 🌈 ☂ ⚡ ❄ ⛄ 🔥 |
| 190 | 🎇 🧨 ✨ 🎈 🎉 🎁 🏆 🏅 ⚽ ⚾ |
| 200 | 🏀 🏐 🏈 🎾 🎳 🏓 🥊 ⛳ ⛸ 🎯 |
| 210 | 🪁 🔮 🎮 🧩 🧸 🪩 🖼 🎨 🧵 🧶 |
| 220 | 🦺 🧣 🧤 🧦 🎒 🩴 👟 👑 👒 🎩 |
| 230 | 🧢 💎 🔔 🎤 📻 🎷 🪗 🎸 🎺 🎻 |
| 240 | 🥁 📺 🔋 💿 ☎ 🕯 💡 📖 📚 📬 |
| 250 | ✏ ✒ 🖌 🖍 📝 💼 📋 📌 📎 🔑 |
| 260 | 🔧 🧲 🪜 🧬 🔭 🩹 🩺 🪞 🛋 🪑 |
| 270 | 🛁 🧹 🧺 🔱 🏁 🐪 🐘 🦃 🍜 🍠 |
| 280 | 🚘 🤿 🃏 👕 📸 🏷 ✂ 🧪 🚪 🧴 |
| 290 | 🧻 🪣 🧽 🚸 🍋‍🟩 🐦‍🔥 🐲 🪅 🦜 🏺 |
| 300 | 🗿 🫐 ⛽ 🍱 🥡 🧋 🍼 📐 |

## Showing the emoji

A terminal may not render every emoji. Images of them are available from Google's Noto
font, e.g. `https://fonts.gstatic.com/s/e/notoemoji/latest/<cp>/emoji.svg`, where `<cp>` is the
lowercase hex code points joined by `_` (`1f62e_200d_1f4a8`).
