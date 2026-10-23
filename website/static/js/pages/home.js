// Home page: a rotating hero headline, and screenshots that move as you scroll.
// Everything here is decoration. Without JavaScript, or with reduced motion
// turned on, the page shows the first headline and still screenshots.
(function () {
  var reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  // ---------------------------------------------------------------- headline
  // The first line is the one in the HTML; the rest follow in a random order.
  var LINES = [
    'Your money, in one file you own.',
    'Money in, money out, in plain English.',
    'Bookkeeping without the subscription.',
    'Budget at home. Keep books for work.',
    'Pay once. Keep it for good.',
    'Tax season, already prepared.',
    'Encrypted with a key only you hold.',
    'No account. No tracking. No catch.'
  ];
  var HOLD_MS = 3800;

  function shuffle(list) {
    for (var i = list.length - 1; i > 0; i--) {
      var j = Math.floor(Math.random() * (i + 1));
      var t = list[i]; list[i] = list[j]; list[j] = t;
    }
    return list;
  }

  function rotateHeadline() {
    var h1 = document.getElementById('hero-title');
    if (!h1) return;
    var order = [LINES[0]].concat(shuffle(LINES.slice(1)));

    // Screen readers and search engines keep the one steady headline. The
    // rotating copies sit in a single grid cell, so the heading is always as
    // tall as its longest line and nothing below it jumps.
    var steady = document.createElement('span');
    steady.className = 'sr-only';
    steady.textContent = h1.textContent;
    var stack = document.createElement('span');
    stack.className = 'hero-rotator';
    stack.setAttribute('aria-hidden', 'true');
    var spans = order.map(function (text, i) {
      var s = document.createElement('span');
      s.textContent = text;
      if (i === 0) s.className = 'is-on';
      stack.appendChild(s);
      return s;
    });
    h1.textContent = '';
    h1.appendChild(steady);
    h1.appendChild(stack);

    var current = 0;
    var timer = null;
    function next() {
      spans[current].classList.remove('is-on');
      current = (current + 1) % spans.length;
      spans[current].classList.add('is-on');
    }
    function start() { if (!timer) timer = setInterval(next, HOLD_MS); }
    function stop() { clearInterval(timer); timer = null; }

    // Hold still while the pointer is on it, and while the tab is hidden.
    h1.addEventListener('mouseenter', stop);
    h1.addEventListener('mouseleave', start);
    document.addEventListener('visibilitychange', function () {
      document.hidden ? stop() : start();
    });
    start();
  }

  // ---------------------------------------------------------------- reveal
  // Screens and cards rise into place the first time they scroll into view.
  function reveal() {
    var targets = document.querySelectorAll(
      '.feature-media, .shot-strip figure, .data-card, .dark-shot, .promise-grid > div'
    );
    if (!('IntersectionObserver' in window)) return;
    document.documentElement.classList.add('motion');
    var seen = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (!entry.isIntersecting) return;
        entry.target.classList.add('is-in');
        seen.unobserve(entry.target);
      });
    }, { rootMargin: '0px 0px -12% 0px' });
    targets.forEach(function (el) {
      // Siblings in a row arrive one after another.
      var parent = el.parentElement;
      var index = Array.prototype.indexOf.call(parent.children, el);
      if (parent.children.length > 1) el.style.transitionDelay = (index % 4) * 90 + 'ms';
      seen.observe(el);
    });
  }

  // ---------------------------------------------------------------- parallax
  // A few screenshots drift at their own pace as the page scrolls past them.
  // data-depth is pixels of travel across the viewport; negative moves up.
  function parallax() {
    var layers = [
      { el: document.querySelector('.hero-shot img'), depth: -40 },
      { el: document.querySelector('.stack-shots img:last-child'), depth: -70 },
      { el: document.querySelector('.stack-shots img:first-child'), depth: 25 },
      { el: document.querySelector('#business .feature-media img'), depth: -35 },
      { el: document.querySelector('#taxes .feature-media img'), depth: -35 },
      { el: document.querySelector('.dark-shot img'), depth: -30 }
    ].filter(function (l) { return l.el; });
    if (!layers.length) return;

    var ticking = false;
    function update() {
      ticking = false;
      var vh = window.innerHeight;
      layers.forEach(function (l) {
        var box = l.el.getBoundingClientRect();
        if (box.bottom < -200 || box.top > vh + 200) return;
        // -1 when the element's centre is at the bottom of the screen, 1 at the top.
        var progress = 1 - ((box.top + box.height / 2) / vh) * 2;
        l.el.style.transform = 'translate3d(0,' + (progress * l.depth).toFixed(1) + 'px,0)';
      });
    }
    function request() {
      if (!ticking) { ticking = true; requestAnimationFrame(update); }
    }
    window.addEventListener('scroll', request, { passive: true });
    window.addEventListener('resize', request);
    update();
  }

  document.addEventListener('DOMContentLoaded', function () {
    if (reduceMotion) return;
    rotateHeadline();
    reveal();
    parallax();
  });
})();
