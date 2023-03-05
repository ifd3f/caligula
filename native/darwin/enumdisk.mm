extern "C" {
#import "enumdisk.h"
}

#import <Cocoa/Cocoa.h>
#import <Foundation/Foundation.h>
#import <DiskArbitration/DiskArbitration.h>

extern DiskList enumerate_disks() {
  DASessionRef session = DASessionCreate(kCFAllocatorDefault);

  // Add mount points
  NSArray *volumeKeys = [NSArray arrayWithObjects:NSURLVolumeNameKey, NSURLVolumeLocalizedNameKey, nil];
  NSArray *volumePaths = [
    [NSFileManager defaultManager]
    mountedVolumeURLsIncludingResourceValuesForKeys:volumeKeys
    options:0
  ];

  DiskList list;
  list.n = 0;
  list.disks = (Disk*)malloc(0);

  for (NSURL *path in volumePaths) {
    DADiskRef disk = DADiskCreateFromVolumePath(kCFAllocatorDefault, session, (__bridge CFURLRef)path);
    if (disk == nil) {
      continue;
    }

    const char *bsdnameChar = DADiskGetBSDName(disk);
    if (bsdnameChar == nil) {
      CFRelease(disk);
      continue;
    }

    NSString *volumeName;
    [path getResourceValue:&volumeName forKey:NSURLVolumeLocalizedNameKey error:nil];

    list.disks = (Disk*)realloc(list.disks, list.n + 1);
    Disk* d = &list.disks[list.n];
    list.n++;

    d->devnode = (char*)malloc(strlen(bsdnameChar) + 1);
    strcpy(d->devnode, bsdnameChar);

    // TODO
    d->model = NULL;
    d->size_is_known = 0;

    CFRelease(disk);
  }
  CFRelease(session);

  return list;
}
