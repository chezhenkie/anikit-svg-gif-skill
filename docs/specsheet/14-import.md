# 14-import

Status: implemented. The import route samples an animated SVG through one browser page and writes the canonical timeline of part 11, so the vector route can feed the tween engine.

## 14.1 What the route adds

`anikit import drawing.svg -o scene.json` reads the motion of a drawing that already animates through CSS, WAAPI or SMIL and writes it as a timeline. The vector route plays such a drawing; the tween route reads a timeline. This route joins the two, and closes the loop from the SVG export back to the model.

It does not parse CSS or SMIL text. The browser resolves selectors, units, timing, easing and precedence, and the route records the five properties that the tween crate supports at the frame times the renderer will use. A value the tween model cannot carry is an error rather than an approximation.

## 14.2 Input and output

- The input must end in `.svg` and exist. HTML pages are not imported, because the timeline names SVG elements.
- The output must end in `.json`. No other extension is accepted.
- The canvas is the drawing's viewport, resolved as on the vector route: `--width` and `--height` override it, and a missing side follows the drawing's own size. The background is `--background`, default `#ffffff`.
- `--duration SEC` is the sampled interval, default 2. `--fps N` is the sample rate and the scene's canvas rate, default 25. The fps must divide 1000 exactly, so the 10 ms grid and the frame count agree: 1, 2, 4, 5, 8, 10, 20, 25, 40 or 50. Any other value fails before the browser starts.
- `--loop-mode once|cycle|pingpong` sets `timing.loop`, default `cycle`. The rate is 1.0.
- `--browser PATH` names the browser, as on the vector route.
- The output is written under a temp name, read back byte for byte and renamed. A failed run leaves no file and no temp file.
- The summary line is `<name>  N tracks, X.XX s, W x H`.

The frame count is `max(2, ceil(duration * fps))`, and the timeline lasts `count / fps` seconds. With an fps that divides 1000, every frame time is a whole number of milliseconds, the frame plan of the tween route reproduces the same count and the same times, and the sampled motion is exact at every rendered frame.

## 14.3 What is imported

- A candidate is every element that the page reports as animated: an element with a CSS or WAAPI animation from `document.getAnimations()`, and the parent of any `animate`, `animateTransform` or `set` element.
- A candidate must have an id, because a track names `file.svg#id`. A candidate without an id, and an animation on the root `svg`, fail the run with a message naming the element. The root cannot be a target (11.7).
- Only five properties are imported: transform, opacity, fill, stroke and stroke-width. An animation of any other property is reported with a `WARN` line when the run writes a scene, and it does not become a track. A drawing whose only animations are outside the five fails with "the drawing has no importable animation" and names the skipped properties.
- A drawing with no candidate at all fails the run.
- A candidate whose property never changes over the sampled interval adds no track: the route compares the samples and writes a track only for motion it saw.

## 14.4 Sampling

The page is loaded once with the same wrapper as the vector route, fonts are awaited, every CSS and WAAPI clock is paused, and the SVG clock is paused. For each frame index `i`, the route seeks every clock to `i * 1000 / fps` milliseconds and reads the computed state of every candidate.

- `opacity`, `fill`, `stroke` and `stroke-width` come from `getComputedStyle` of the element.
- The transform is the element's own rendered transform. When a CSS transform is in effect, its matrix is read from the computed style; otherwise the XML transform presentation value is read from the SVG DOM, which includes SMIL. The two are distinguished by comparing the computed matrix with the presentation matrix.
- The tween route puts its matrix on a wrapper outside the element and leaves the element's static `transform` attribute in place. The sampled matrix is therefore multiplied by the inverse of the static attribute matrix: a track moves the element from its authored transform, not from the identity. The static matrix is read from the element's `transform` attribute in the page and parsed with the SVG transform list grammar: `matrix`, `translate`, `scale`, `rotate` with an optional centre, `skewX` and `skewY`, in list order. An attribute the parser does not understand fails the run and names the element.
- A CSS transform is applied about `transform-origin`. The route accepts only an origin of `0px 0px` and fails otherwise, naming the element; the exported CSS of part 12 sets exactly that origin. An XML transform is always about 0 0 and needs no correction.
- A sampled matrix is decomposed into translate, rotate, scale and skew, the components of part 11's transform value. The decomposition is the inverse of the composition in the tween crate, so the matrix survives the round trip. Negative scales and rotations beyond a half turn are reported by the decomposition without loss.
- A colour that never changes may be `none`, a paint server or a partially transparent value; it simply never becomes a track. A colour that changes must be opaque sRGB, because the model has no alpha-only colour, no gradient and no paint-off value (11.2.2, 07.4). A changing colour that is `none`, a paint server or not opaque fails the run and names the element.
- `opacity` is a number in 0 to 1, `stroke-width` a non-negative number in user units. A percentage or a unit the route cannot resolve fails.

## 14.5 The scene

The output is the canonical object of 11.2 with these values:

- `version` 1; `canvas` carries the resolved width and height, `--background` and the import fps.
- `timing` carries the chosen loop mode and `rate` 1.0.
- One track per changed property, with id `<element>-<property>`, where property is `transform`, `opacity`, `fill`, `stroke` or `stroke-width`. The kind is the matching `svg.*`.
- `src` is the drawing path relative to the scene file, with forward slashes, plus `#<element>`. When no relative path exists, the absolute path is written.
- Keys sit at every frame time in whole milliseconds, plus one final key at the loop end that repeats the last sample. That key holds the last value through the unrendered tail of the loop, as the SVG export does.
- Every key except the final hold carries `"easing": "linear"`: the samples already contain the source easing, so a segment between two samples must not curve. `smooth` is absent.
- Colours are `#rrggbb`, numbers are rounded to six decimals, and the order of tracks follows the page order of their elements.
- `layers` is always empty: the route records the composited result, not the source's own layers.

## 14.6 Errors

The route reports, with one `FAIL` line and no output:

- the input is missing or not `.svg`, or the output is not `.json`;
- the fps does not divide 1000, or another option is out of range;
- no Chromium based browser is found or the page does not load;
- the drawing has no animation, or no animation of the five properties;
- an animated element has no id, or the root `svg` is animated;
- a transform has a non-zero `transform-origin`, or a static transform attribute cannot be parsed;
- a colour is `none` or not opaque while it is animating;
- the browser returns a sample the route cannot place, or the file does not read back.

## 14.7 Tests

- A CSS drawing with a transform and an opacity animation imports to two tracks at the expected times, canvas and fps. Every sampled transform matches the matrix the drawing asks for, and the scene renders through the tween route with the element moving.
- A SMIL drawing with `animate` and `animateTransform` imports the same way, including a static transform attribute that the wrapper must divide out.
- A scene exported by the tween route is imported back: track count, duration, loop mode and sampled values match the source plan, and the imported scene plays through the tween route.
- Rendering an imported scene does not replay the source drawing's animation: a drawing whose animation starts 40 px from its own position renders with the box at the sampled 40, not at 80.
- A drawing with no animation, an animated element without an id, a non-zero transform origin, a `none` colour and an unsupported fps each fail with one line and leave no output.
- A drawing whose only animation is `x` or `d` is reported as not importable.

## 14.8 Limits

- The route samples, it does not parse. The source easing is exact at the imported frame times and linear between them, so an imported scene is rendered at its own fps for an exact match. A higher render fps shows the straight segments.
- Only the five properties. Position, size, path data, text content, paint servers and colour alpha stay with their open parts (06, 07) and are not imported.
- One interval. Animations with different periods are sampled over `--duration` and the scene repeats that interval; several independent periods cannot be represented.
- A JavaScript loop that writes attributes or CSS by hand is not an animation object, so it is not seen. CSS, WAAPI and declarative SMIL are.
- A non-zero CSS `transform-origin` is refused rather than approximated.
