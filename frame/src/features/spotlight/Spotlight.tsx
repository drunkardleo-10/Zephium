export function Spotlight() {
  return (
    <div class="flex h-screen w-screen items-start justify-center p-3">
      <input
        autofocus
        spellcheck={false}
        placeholder="Search or run a command"
        class="h-12 w-full rounded-xl bg-elevated/80 px-4 text-[15px] text-text outline-none placeholder:text-faint"
      />
    </div>
  );
}
