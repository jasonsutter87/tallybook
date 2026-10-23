// Download page: highlight the installer for the visitor's computer.
document.addEventListener('DOMContentLoaded', function () {
  var ua = navigator.userAgent;
  var os = /Mac/.test(ua) ? 'mac' : /Win/.test(ua) ? 'windows' : /Linux|X11/.test(ua) ? 'linux' : null;
  if (!os) return;
  var card = document.querySelector('.os-card[data-os="' + os + '"]');
  if (card) card.classList.add('is-detected');
});
