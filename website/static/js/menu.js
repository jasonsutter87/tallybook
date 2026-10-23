// Mobile menu: the toggle button opens and closes it, and following a link
// closes it. Kept in a file (not inline handlers) so the site's
// Content-Security-Policy can allow scripts from this origin only.
(function () {
  var menu = document.getElementById('mobile-menu');
  var toggle = document.querySelector('[aria-controls="mobile-menu"]');
  if (!menu || !toggle) return;

  toggle.addEventListener('click', function () {
    var open = menu.classList.toggle('hidden') === false;
    toggle.setAttribute('aria-expanded', open ? 'true' : 'false');
  });

  menu.addEventListener('click', function (event) {
    if (event.target.closest('a')) {
      menu.classList.add('hidden');
      toggle.setAttribute('aria-expanded', 'false');
    }
  });
})();
