`bendfm-0.stp.gz` is part 0 from the sample directory of
[BenDFM](https://github.com/UGent-CVAMO/bendfm/tree/main/example_files), distributed under the MIT
License. Copyright (c) 2025 Matteo Ballegeer.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

The other fixtures are original synthetic Apache-2.0 parts. They use
1 mm stock, quarter-turn bends, and 2 mm inner radii.

- `overlapping-flanges.step.gz`: an L-shaped base with straight flanges on
  neighboring inside edges. One folds upward and one downward. Their 2D
  developments both extend into the missing quadrant.
- `coaxial-flanges.step.gz`: two chains of two bends, at different axial
  stations on opposite sides of one base. Their final faces lie in the same
  plane and the return bends are coaxial. The two branches remain separate.
- `coaxial-overlap.step.gz`: the same coaxial returns with an additional
  downturned flange. Its development crosses one return branch, so the
  split interpretation alone cannot prove a flat blank.
