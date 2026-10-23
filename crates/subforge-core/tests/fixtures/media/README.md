# sample.mkv

Synthetic, redistributable test clip (about 33 KB): 5 s, 160x90 H.264 video
(ffmpeg `testsrc`), stereo AAC audio (440 Hz sine), an ASS subtitle track
tagged `ara` (one short Arabic line, style font "Noto Naskh Arabic", no
embedded font) and an SRT track tagged `eng`.

Created with ffmpeg and mkvmerge (MKVToolNix v102):

```sh
# ara.ass: a minimal [Script Info] / [V4+ Styles] / [Events] file with one line:
#   Dialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,مرحبا بالعالم
printf '1\n00:00:01,000 --> 00:00:03,000\nHello world\n' > eng.srt

ffmpeg -v error -y -f lavfi -i testsrc=size=160x90:rate=10:duration=5 \
  -f lavfi -i sine=frequency=440:duration=5 -ac 2 \
  -c:v libx264 -preset veryfast -crf 35 -pix_fmt yuv420p \
  -c:a aac -b:a 32k -ar 16000 base.mkv

mkvmerge -o sample.mkv base.mkv \
  --language 0:ara --track-name 0:Arabic ara.ass \
  --language 0:eng --track-name 0:English eng.srt
```

Resulting mkvmerge track ids: 0 video, 1 audio, 2 subtitles `ara`, 3 subtitles `eng`.
