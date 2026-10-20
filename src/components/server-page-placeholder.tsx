import {
  Card,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@sunstead/ui/components/card';
import { SERVER_PAGES, type ServerPage } from './server-navigation';

export function ServerPagePlaceholder({ page }: { page: ServerPage }) {
  const currentPage = SERVER_PAGES.find((item) => item.id === page);

  if (!currentPage) {
    return null;
  }

  return (
    <div className='mx-auto w-full max-w-[1600px] p-4 md:p-6'>
      <Card>
        <CardHeader>
          <CardTitle>{currentPage.label}</CardTitle>
          <CardDescription>
            {currentPage.label} management will be available here.
          </CardDescription>
        </CardHeader>
      </Card>
    </div>
  );
}
