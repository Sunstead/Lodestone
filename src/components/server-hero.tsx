import { Button } from '@sunstead/ui/components/button';
import { Box } from 'lucide-react';
import backgroundImg from '@/assets/wallpaper/wallpaper_minecraft_pc_bundle_2560x1440.png';
import Play from '@/assets/icons/play.svg?react';
import type { ServerId } from './server-navigation';
import { SERVERS } from './server-navigation';

export function ServerHero({ serverId }: { serverId: ServerId }) {
  const server = SERVERS.find(({ id }) => id === serverId)!;

  return (
    <header
      className='relative flex min-h-64 w-full flex-col justify-end bg-cover bg-center pb-14'
      style={{ backgroundImage: `url(${backgroundImg})` }}
    >
      <div
        aria-hidden='true'
        className='absolute inset-0 z-0 bg-linear-to-r from-black/40 via-transparent to-black/40'
      />
      <div className='z-10 my-auto flex flex-wrap items-center gap-4 p-4'>
        <div className='flex min-w-0 items-center gap-4'>
          <Box aria-hidden='true' className='size-20 shrink-0 sm:size-32' />
          <div className='min-w-0'>
            <h1 className='truncate text-4xl font-bold text-shadow-lg sm:text-6xl'>
              {server.name}
            </h1>
            <p className='text-xl font-semibold text-muted-foreground text-shadow-lg'>
              {server.version}
            </p>
          </div>
        </div>
        <Button className='bevel bevel-main ml-auto bg-(--bevel-base) px-8 py-6 text-lg font-semibold uppercase text-accent-foreground hover:bg-(--bevel-base)'>
          <Play aria-hidden='true' className='size-6' />
          Start
        </Button>
      </div>
    </header>
  );
}
