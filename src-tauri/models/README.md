# Офлайн-модели перевода

Модели RU→EN и EN→RU проекта Firefox Translations (Mozilla, движок Bergamot / Marian),
лицензия [MPL-2.0](https://www.mozilla.org/MPL/2.0/). Скачаны из Remote Settings Firefox
(коллекция `translations-models`), контрольные суммы SHA-256 сверены с записями коллекции:

| Файл | Исходное имя | Версия | SHA-256 (несжатого файла) |
|---|---|---|---|
| ruen/model.bin.gz | model.ruen.intgemm.alphas.bin | 2.1 | aa0efa194b3dbd2da129ba0b8db8ef139126cb0914e57072f62061294746f1e2 |
| ruen/lex.bin.gz | lex.50.50.ruen.s2t.bin | 2.1 | c944e753f298bbdf6506388de7257fb48cbd12c83e2807e2c284fc323771895b |
| ruen/vocab.spm.gz | vocab.ruen.spm | 2.1 | de8af5f87e231ee4016cec4bb482e6b584cb1ca34dcb624fc7b8398f25bb35df |
| enru/model.bin.gz | model.enru.intgemm.alphas.bin | 2.1a1 | 184cb5cda528eeefc0f75f5d0035d787b71d74af135e3c5608d01ae02ecfb920 |
| enru/lex.bin.gz | lex.50.50.enru.s2t.bin | 2.1a1 | 4d91839726b960e70b6d05c53d0cffd16262832b1c0e1ea99d66f412dcc6a239 |
| enru/vocab.spm.gz | vocab.enru.spm | 2.1a1 | 56ee63e14e8cb926c394242adc3ed7cc602644c3d33058cff2ce2959d52a6258 |

Файлы сжаты gzip и встраиваются в `notch.exe` (`include_bytes!`), в окне распаковываются
через `DecompressionStream`. Сеть для перевода не нужна.
