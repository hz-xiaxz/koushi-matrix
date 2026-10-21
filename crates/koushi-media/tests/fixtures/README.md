# Synthetic color profile

`srgb.icc` was generated with LittleCMS through Pillow's
`ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB")).tobytes()`.
Its ICC header date (bytes 24–35) is normalized to 2000-01-01 00:00:00.
The generated profile's copyright tag says “No copyright, use freely”.
It contains no image, account, or user metadata.

The separate `heif/` directory carries its own fixture provenance and license.

`animated.webp` was generated with Pillow: two solid 32x24 RGBA frames (red and
green), each lasting 100 ms, encoded losslessly with infinite looping. It carries
no source image or private metadata.
