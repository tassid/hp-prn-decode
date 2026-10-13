# hp-prn-decode

Lê um job de impressão HP PCL3GUI capturado em arquivo (`.prn`) e reconstrói a página como PNG.

Feito para entender como o driver da HP Deskjet F4400 fala com a impressora. Testado com uma captura desse driver.

## Uso

```bash
cargo run --release -- job.prn pagina.png
```

Para capturar um job no Windows sem imprimir: mande imprimir com "Imprimir em arquivo" pelo driver da impressora.

Atenção: o `.prn` leva no cabeçalho PJL o nome do usuário, do computador e do programa que imprimiu. Não publique capturas sem olhar.

## O que ele entende

- Sequências de escape PCL, inclusive as encadeadas (`ESC*b0v13W…`).
- Largura do raster (`ESC*r#S`), salto de linhas em branco (`ESC*b#Y`), linhas do plano de cor (`ESC*b#W`).
- Compressão "Mode 10" das linhas, em [src/mode10.rs](src/mode10.rs).

## Mode 10 em resumo

Cada linha é uma lista de comandos. Um comando pula N pixels, que ficam iguais aos da linha de cima, e depois pinta um trecho: uma cor repetida ou pixels soltos. A cor do primeiro pixel vem de um valor novo, do pixel à esquerda, do pixel acima à direita ou da última cor nova usada. Linha sem bytes é cópia da linha de cima.

## Limites

- Só desenha o plano de cor. Bytes do plano preto (`ESC*b#V`) são contados e informados, não desenhados.
- O driver observado termina algumas linhas 12 pixels além da largura declarada; o excesso é cortado.
- Testado com uma página de texto e retângulos. Fotos não foram testadas.
