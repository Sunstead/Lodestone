import { SidebarServerButton } from '#/components/sidebar-server-button';
import { Button } from '@sunstead/ui/components/button';
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarRail,
} from '@sunstead/ui/components/resizable-sidebar';
import { ThemeProvider } from '@sunstead/ui/theme-provider';
import { createFileRoute } from '@tanstack/react-router';
import {
  Box,
  Container,
  File,
  FileText,
  Folder,
  HomeIcon,
  MoreVertical,
  Play,
  Plus,
  PlusCircle,
  Puzzle,
  Settings,
  Terminal,
  User,
  UserGroup,
} from 'lucide-react';
import backgroundImg from '@/assets/wallpaper/wallpaper_minecraft_pc_bundle_2560x1440.png';

export const Route = createFileRoute('/')({ component: Home });

function Home() {
  return (
    <div className='w-screen h-screen'>
      <ThemeProvider app='lodestone'>
        <SidebarProvider>
          <Sidebar>
            <SidebarHeader className='text-xl font-semibold border-b'>
              <div className='py-1.5 px-2'>LODESTONE</div>
            </SidebarHeader>
            <SidebarContent>
              <SidebarGroup>
                <SidebarMenu className='gap-2 px-2'>
                  <Button className='bevel py-6 text-lg font-semibold uppercase text-accent-foreground bg-(--bevel-base) hover:bg-(--bevel-base) bevel-main'>
                    <Plus className='size-6' />
                    Create Server
                  </Button>
                  {/* <Button className='bevel py-6 text-lg font-semibold uppercase text-accent-foreground bg-(--bevel-base) hover:bg-(--bevel-base) bevel-gray justify-start'>
                    <Folder className='size-6' /> Add Existing
                  </Button> */}
                </SidebarMenu>
              </SidebarGroup>
              <SidebarGroup>
                <SidebarGroupLabel>Servers</SidebarGroupLabel>
                <SidebarMenu className='px-2 space-y-1'>
                  <SidebarServerButton />
                  <SidebarServerButton />
                  <SidebarServerButton />
                </SidebarMenu>
              </SidebarGroup>
            </SidebarContent>
            <SidebarRail />
          </Sidebar>
          <SidebarInset>
            <div
              className='min-h-80 w-full relative bg-cover bg-center flex flex-col justify-between'
              style={{
                backgroundImage: `url(${backgroundImg})`,
              }}
            >
              <div className='absolute inset-0 bg-linear-to-r from-black/40 to-black/40 via-transparent z-0'></div>

              <div className='flex items-center w-full bg-sidebar/80 backdrop-blur-md p-2 border-b'>
                <div className='size-10 bg-accent rounded-full ml-auto'></div>
              </div>

              <div className='flex items-center p-4 z-10'>
                <div className='flex items-center gap-x-4'>
                  <Box className='size-32 min-w-32' />
                  <div>
                    <p className='text-6xl font-bold'>Survival</p>
                    <p className='text-xl text-muted-foreground text-shadow-md font-semibold'>
                      Fabric 26.3
                    </p>
                  </div>
                </div>
                <div className='flex items-center ml-auto'>
                  <Button className='bevel py-6 px-8 text-lg font-semibold uppercase text-accent-foreground bg-(--bevel-base) hover:bg-(--bevel-base) bevel-main'>
                    <Play className='size-6' /> Start
                  </Button>
                </div>
              </div>

              <div className='flex items-center bg-sidebar/80 backdrop-blur-md border-t border-b w-full'>
                <Button
                  variant='ghost'
                  className='p-4 py-4 h-max shadow-[inset_0_-4px_0_0_var(--color-main)]'
                >
                  <HomeIcon /> Dashboard
                </Button>
                <Button
                  variant='ghost'
                  className='p-4 py-4 h-max active:shadow-[inset_0_-4px_0_0_var(--color-main)]'
                >
                  <Terminal /> Console
                </Button>
                <Button
                  variant='ghost'
                  className='p-4 py-4 h-max active:shadow-[inset_0_-4px_0_0_var(--color-main)]'
                >
                  <FileText /> Server Properties
                </Button>
                <Button
                  variant='ghost'
                  className='p-4 py-4 h-max active:shadow-[inset_0_-4px_0_0_var(--color-main)]'
                >
                  <Puzzle /> Mods
                </Button>
                <Button
                  variant='ghost'
                  className='p-4 py-4 h-max active:shadow-[inset_0_-4px_0_0_var(--color-main)]'
                >
                  <UserGroup /> Players
                </Button>
                <Button
                  variant='ghost'
                  className='p-4 py-4 h-max active:shadow-[inset_0_-4px_0_0_var(--color-main)]'
                >
                  <Container /> Backups
                </Button>
                <Button
                  variant='ghost'
                  className='p-4 py-4 h-max active:shadow-[inset_0_-4px_0_0_var(--color-main)]'
                >
                  <Settings /> Settings
                </Button>
              </div>
            </div>
          </SidebarInset>
        </SidebarProvider>
      </ThemeProvider>
    </div>
  );
}
