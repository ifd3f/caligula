// A lot of it was borrowed from: https://github.com/balena-io-modules/drivelist/blob/master/src/darwin/list.mm

extern "C" {
#import "enumdisk.h"
}

#import "REDiskList.h"

#import <Cocoa/Cocoa.h>
#import <Foundation/Foundation.h>
#import <DiskArbitration/DiskArbitration.h>

NSNumber *DictionaryGetNumber(CFDictionaryRef dict, const void *key) {
  return (NSNumber*)CFDictionaryGetValue(dict, key);
}

bool IsDiskPartition(NSString *disk) {
  NSPredicate *partitionRegEx = [NSPredicate predicateWithFormat:@"SELF MATCHES %@", @"disk\\d+s\\d+"];
  return [partitionRegEx evaluateWithObject:disk];
}

char *MakeOwnedCStr(const char *s) {
  char* out = (char*)malloc(strlen(s) + 1);
  strcpy(out, s);
  return out;
}

void appendDiskInfo(DiskList *list, const char *bsdname, int dev_type, CFDictionaryRef diskinfo) {
  list->disks = (Disk*)realloc(list->disks, (list->n + 1) * sizeof(Disk));
  Disk* d = &list->disks[list->n];
  list->n++;

  d->bsdname = MakeOwnedCStr(bsdname);

  // For keys, see: https://developer.apple.com/documentation/diskarbitration/diskarbitration_constants

  NSString *description = (NSString*)CFDictionaryGetValue(diskinfo, kDADiskDescriptionMediaNameKey);
  if (description != nil) {
    const char* description_bytes = [description UTF8String];
    d->model = MakeOwnedCStr(description_bytes);
  } else {
    d->model = 0;
  }

  bool removable = [DictionaryGetNumber(diskinfo, kDADiskDescriptionMediaRemovableKey) boolValue];
  bool ejectable = [DictionaryGetNumber(diskinfo, kDADiskDescriptionMediaEjectableKey) boolValue];
  d->is_removable = removable || ejectable;

  d->size_is_known = 1;
  d->size = [DictionaryGetNumber(diskinfo, kDADiskDescriptionMediaSizeKey) unsignedLongValue];

  NSNumber *blockSize = DictionaryGetNumber(diskinfo, kDADiskDescriptionMediaBlockSizeKey);
  d->block_size_is_known = 1;
  d->block_size = [blockSize unsignedLongValue];

  d->dev_type = dev_type;
}

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

  // Enumerate root disks
  REDiskList *dl = [[REDiskList alloc] init];
  for (NSString* diskBsdName in dl.disks) {
    int type = IsDiskPartition(diskBsdName) ? DEV_TYPE_PARTITION : DEV_TYPE_DISK;

    const char *bsdname = [diskBsdName UTF8String];
    DADiskRef disk = DADiskCreateFromBSDName(kCFAllocatorDefault, session, bsdname);
    if (disk == nil) {
      continue;
    }

    CFDictionaryRef diskinfo = DADiskCopyDescription(disk);
    if (diskinfo == nil) {
      CFRelease(disk);
      continue;
    }

    appendDiskInfo(&list, bsdname, type, diskinfo);

    CFRelease(diskinfo);
    CFRelease(disk);
  }
  [dl release];

  // Enumerate disk volumes
  for (NSURL *path in volumePaths) {
    DADiskRef disk = DADiskCreateFromVolumePath(kCFAllocatorDefault, session, (__bridge CFURLRef)path);
    if (disk == nil) {
      continue;
    }

    const char *bsdname = DADiskGetBSDName(disk);
    if (bsdname == nil) {
      CFRelease(disk);
      continue;
    }

    CFDictionaryRef diskinfo = DADiskCopyDescription(disk);
    if (diskinfo == nil) {
      CFRelease(disk);
      continue;
    }

    appendDiskInfo(&list, bsdname, DEV_TYPE_PARTITION, diskinfo);

    CFRelease(diskinfo);
    CFRelease(disk);
  }
  CFRelease(session);

  return list;
}
