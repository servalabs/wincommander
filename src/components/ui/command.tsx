import * as React from "react"
import { Command as CommandPrimitive } from "cmdk"
import { Search } from "lucide-react"
import { cn } from "../../lib/utils"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "./dialog"

function Command({
  className,
  ...props
}: React.ComponentProps<typeof CommandPrimitive>) {
  return (
    <CommandPrimitive
      data-slot="command"
      className={cn(
        "flex h-full w-full flex-col overflow-hidden rounded-[var(--r-lg)] bg-[var(--surface)] text-[var(--text)]",
        className,
      )}
      {...props}
    />
  )
}

function CommandDialog({
  title = "Command Palette",
  description = "Search for a command to run...",
  children,
  ...props
}: React.ComponentProps<typeof Dialog> & {
  title?: string
  description?: string
}) {
  return (
    <Dialog {...props}>
      <DialogTitle className="sr-only">{title}</DialogTitle>
      <DialogDescription className="sr-only">{description}</DialogDescription>
      <DialogContent className="overflow-hidden p-0 max-w-2xl data-[state=open]:slide-in-from-top-3 data-[state=open]:duration-[100ms] data-[state=closed]:slide-out-to-top-3 data-[state=closed]:duration-[60ms]">
        <Command className="[&_[cmdk-group-heading]]:text-[var(--text-mute)] [&_[cmdk-input-wrapper]_svg]:size-[13px]">
          {children}
        </Command>
      </DialogContent>
    </Dialog>
  )
}

function CommandInput({
  className,
  ...props
}: React.ComponentProps<typeof CommandPrimitive.Input>) {
  return (
    <div
      data-slot="command-input-wrapper"
      className="flex items-center gap-2.5 border-b border-[var(--border)] px-3 py-2.5"
      cmdk-input-wrapper=""
    >
      <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-[var(--r)] bg-[var(--accent-soft)]">
        <Search className="size-[13px] shrink-0 text-[var(--accent)]" />
      </div>
      <CommandPrimitive.Input
        data-slot="command-input"
        className={cn(
          "h-10 w-full bg-transparent text-[13px] text-[var(--text)] outline-none placeholder:text-[var(--text-mute)] disabled:cursor-not-allowed disabled:opacity-50",
          className,
        )}
        {...props}
      />
    </div>
  )
}

function CommandList({
  className,
  ...props
}: React.ComponentProps<typeof CommandPrimitive.List>) {
  return (
    <CommandPrimitive.List
      data-slot="command-list"
      className={cn(
        "max-h-[min(60vh,560px)] overflow-y-auto overflow-x-hidden scroll-smooth p-1.5 [&::-webkit-scrollbar]:w-[3px] [&::-webkit-scrollbar-thumb]:rounded-sm [&::-webkit-scrollbar-thumb]:bg-[var(--accent-soft)]",
        className,
      )}
      {...props}
    />
  )
}

function CommandEmpty({
  className,
  ...props
}: React.ComponentProps<typeof CommandPrimitive.Empty>) {
  return (
    <CommandPrimitive.Empty
      data-slot="command-empty"
      className={cn("py-6 text-center text-[13px] text-[var(--text-mute)]", className)}
      {...props}
    />
  )
}

function CommandGroup({
  className,
  ...props
}: React.ComponentProps<typeof CommandPrimitive.Group>) {
  return (
    <CommandPrimitive.Group
      data-slot="command-group"
      className={cn(
        "p-1 text-[var(--text)] [&_[cmdk-group-heading]]:mb-0.5 [&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1 [&_[cmdk-group-heading]]:text-[10px] [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wider [&_[cmdk-group-heading]]:text-[var(--text-mute)] [&_[cmdk-group-heading]]:font-[family-name:var(--font-mono)]",
        className,
      )}
      {...props}
    />
  )
}

function CommandSeparator({
  className,
  ...props
}: React.ComponentProps<typeof CommandPrimitive.Separator>) {
  return (
    <CommandPrimitive.Separator
      data-slot="command-separator"
      className={cn("mx-1.5 my-1 h-px bg-[var(--border)]", className)}
      {...props}
    />
  )
}

function CommandItem({
  className,
  ...props
}: React.ComponentProps<typeof CommandPrimitive.Item>) {
  return (
    <CommandPrimitive.Item
      data-slot="command-item"
      className={cn(
        "group flex cursor-pointer select-none items-center gap-2.5 rounded-[var(--r)] px-2 py-1.5 text-[13px] outline-none transition-[background,box-shadow] duration-150 data-[selected=true]:bg-[var(--accent-soft)] data-[selected=true]:shadow-[0_0_14px_-5px_var(--glow)] data-[disabled=true]:pointer-events-none data-[disabled=true]:opacity-50 [&_svg]:shrink-0",
        className,
      )}
      {...props}
    />
  )
}

function CommandShortcut({
  className,
  ...props
}: React.ComponentProps<"span">) {
  return (
    <span
      data-slot="command-shortcut"
      className={cn(
        "ml-auto text-[10px] tracking-wider text-[var(--text-mute)] font-[family-name:var(--font-mono)]",
        className,
      )}
      {...props}
    />
  )
}

function CommandFooter({
  className,
  ...props
}: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="command-footer"
      className={cn(
        "flex items-center justify-center gap-4 border-t border-[var(--border)] px-3 py-2",
        className,
      )}
      {...props}
    />
  )
}

export {
  Command,
  CommandDialog,
  CommandInput,
  CommandList,
  CommandEmpty,
  CommandGroup,
  CommandItem,
  CommandShortcut,
  CommandSeparator,
  CommandFooter,
}
