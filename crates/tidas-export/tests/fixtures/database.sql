CREATE TABLE ilcd (
  file_name text PRIMARY KEY,
  json_ordered jsonb NOT NULL
);

CREATE TABLE contacts (
  id uuid NOT NULL,
  json_ordered jsonb NOT NULL,
  version text NOT NULL,
  state_code integer NOT NULL
);
CREATE TABLE flows (LIKE contacts INCLUDING ALL);
CREATE TABLE flowproperties (LIKE contacts INCLUDING ALL);
CREATE TABLE processes (LIKE contacts INCLUDING ALL);
CREATE TABLE sources (LIKE contacts INCLUDING ALL);
CREATE TABLE unitgroups (LIKE contacts INCLUDING ALL);
CREATE TABLE lciamethods (LIKE contacts INCLUDING ALL);
CREATE TABLE lifecyclemodels (LIKE contacts INCLUDING ALL);

INSERT INTO ilcd VALUES (
  'ILCDLocations',
  '{"ILCDLocations":{"location":[{"@id":"CN"}]}}'
);
INSERT INTO contacts VALUES (
  '11111111-1111-1111-1111-111111111111',
  '{"contactDataSet":{"common:UUID":"11111111-1111-1111-1111-111111111111"}}',
  '01.00.005',
  100
);
INSERT INTO contacts VALUES (
  '11111111-1111-1111-1111-111111111111',
  '{"contactDataSet":{"common:UUID":"11111111-1111-1111-1111-111111111111"}}',
  '01.00.006',
  100
);
INSERT INTO flows VALUES (
  '22222222-2222-2222-2222-222222222222',
  '{"flowDataSet":{"common:UUID":"22222222-2222-2222-2222-222222222222","administrativeInformation":{"publicationAndOwnership":{"common:referenceToOwnershipOfDataSet":{"@refObjectId":"11111111-1111-1111-1111-111111111111","@type":"contact data set","@version":"01.00.005","@uri":"../contacts/11111111-1111-1111-1111-111111111111_01.00.005.xml"}}}}}',
  '01.01.000',
  100
);
INSERT INTO processes VALUES (
  '33333333-3333-3333-3333-333333333333',
  '{"processDataSet":{"common:UUID":"33333333-3333-3333-3333-333333333333"}}',
  '01.01.000',
  100
);
