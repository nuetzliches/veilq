<script setup lang="ts">
// The mark in the top bar. On hover it turns over to the nut of the nuts
// family (Lucide's `nut`, ISC), as in their other apps; the favicon turns
// with it.
const FRONT = '/favicon.svg';
const BACK = '/logo-alternate.svg';

function favicon(href: string) {
  document.querySelector<HTMLLinkElement>('link[rel="icon"]')?.setAttribute('href', href);
}
</script>

<template>
  <span class="mark" data-testid="brand-mark" @mouseenter="favicon(BACK)" @mouseleave="favicon(FRONT)">
    <span class="mark-inner">
      <img :src="FRONT" alt="" class="face" />
      <img :src="BACK" alt="" class="face back" />
    </span>
  </span>
</template>

<style scoped>
.mark {
  display: inline-block;
  flex: none;
  width: 22px;
  height: 22px;
  perspective: 600px;
}
.mark-inner {
  display: block;
  position: relative;
  width: 100%;
  height: 100%;
  transition: transform 0.55s cubic-bezier(0.4, 0, 0.2, 1);
  transform-style: preserve-3d;
}
.mark:hover .mark-inner {
  transform: rotateY(180deg);
}
.face {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  backface-visibility: hidden;
}
.back {
  transform: rotateY(180deg);
}
@media (prefers-reduced-motion: reduce) {
  .mark-inner {
    transition: none;
  }
}
</style>
